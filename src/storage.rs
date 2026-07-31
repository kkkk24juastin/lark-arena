//! 狼人杀房间的 redb 持久化。

use crate::util::FoldHashMap;
use crate::werewolf::WolfGame;
use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::Serialize;
use std::{path::Path, sync::Arc};
use tracing::warn;

const WOLF_SCHEMA_VERSION: u32 = 1;
const WOLF_GAMES: TableDefinition<&str, &[u8]> = TableDefinition::new("wolf_games");

#[derive(Serialize)]
struct Envelope<'a, T> {
    v: u32,
    data: &'a T,
}

#[derive(serde::Deserialize)]
struct EnvelopeOwned<T> {
    v: Option<u32>,
    data: Option<T>,
}

pub struct Store {
    db: Database,
}

impl Store {
    pub fn open(path: &Path) -> Result<Arc<Self>> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }
        let db = Database::create(path)
            .with_context(|| format!("opening redb at {}", path.display()))?;
        let txn = db.begin_write()?;
        let _ = txn.open_table(WOLF_GAMES)?;
        txn.commit()?;
        Ok(Arc::new(Self { db }))
    }

    pub fn save_wolf(&self, chat_id: &str, game: &WolfGame) -> Result<()> {
        let bytes = sonic_rs::to_vec(&Envelope {
            v: WOLF_SCHEMA_VERSION,
            data: game,
        })?;
        let txn = self.db.begin_write()?;
        txn.open_table(WOLF_GAMES)?
            .insert(chat_id, bytes.as_slice())?;
        txn.commit()?;
        Ok(())
    }

    pub fn delete_wolf(&self, chat_id: &str) -> Result<()> {
        let txn = self.db.begin_write()?;
        txn.open_table(WOLF_GAMES)?.remove(chat_id)?;
        txn.commit()?;
        Ok(())
    }

    pub fn load_all_wolf(&self) -> Result<FoldHashMap<String, WolfGame>> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(WOLF_GAMES)?;
        let mut games = FoldHashMap::default();
        for entry in table.iter()? {
            let (key, value) = entry?;
            let chat_id = key.value().to_string();
            match decode(value.value()) {
                Ok(game) => {
                    games.insert(chat_id, game);
                }
                Err(e) => warn!(?e, chat_id = %chat_id, "skipping invalid werewolf record"),
            }
        }
        Ok(games)
    }
}

fn decode(bytes: &[u8]) -> Result<WolfGame> {
    if let Ok(envelope) = sonic_rs::from_slice::<EnvelopeOwned<WolfGame>>(bytes) {
        if let Some(version) = envelope.v {
            let game = envelope.data.context("werewolf envelope missing data")?;
            if version > WOLF_SCHEMA_VERSION {
                anyhow::bail!(
                    "werewolf schema v{version} is newer than this binary's v{WOLF_SCHEMA_VERSION}"
                );
            }
            return Ok(game);
        }
    }
    sonic_rs::from_slice(bytes).context("failed to deserialize werewolf record")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn werewolf_room_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("arena.redb")).unwrap();
        let mut game = WolfGame::new("oc_test".into());
        game.add_player("ou_1".into(), "玩家一".into()).unwrap();

        store.save_wolf("oc_test", &game).unwrap();

        let loaded = store.load_all_wolf().unwrap();
        let restored = loaded.get("oc_test").unwrap();
        assert_eq!(restored.players.len(), 1);
        assert_eq!(restored.players[0].open_id, "ou_1");
    }
}
