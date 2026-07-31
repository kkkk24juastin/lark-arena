use crate::config::Config;
use crate::feishu::cards::*;
use crate::feishu::events::{BotAdded, CardAction, InboundMessage, MemberAdded, Mention};
use crate::llm::LlmClient;
use crate::persona::Persona;
use crate::storage::Store;
use crate::util::FoldHashMap;
use crate::werewolf::{WolfGame, game::Stage};
use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};

pub struct Bot {
    pub client: Arc<crate::feishu::Client>,
    pub(crate) cfg: Config,
    pub(crate) wolf_games: Mutex<FoldHashMap<String, WolfGame>>,
    pub(crate) bot_open_id: Mutex<Option<String>>,
    pub(crate) seen_events: Mutex<FoldHashMap<String, Instant>>,
    pub(crate) seen_actions: Mutex<FoldHashMap<u64, Instant>>,
    pub(crate) llm: Option<LlmClient>,
    pub(crate) store: Arc<Store>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    WolfJoin,
    WolfLeave,
    WolfStart,
    WolfReset,
    WolfHelp,
}

impl Bot {
    pub fn new(client: Arc<crate::feishu::Client>, cfg: Config, store: Arc<Store>) -> Arc<Self> {
        let llm = cfg.openai_api_key.clone().map(|key| {
            info!(
                model = %cfg.openai_model,
                reasoning_effort = ?cfg.openai_reasoning_effort,
                "LLM AI seats enabled"
            );
            LlmClient::new(
                key,
                cfg.openai_base_url.clone(),
                cfg.openai_model.clone(),
                cfg.openai_reasoning_effort.clone(),
            )
        });
        let wolf_games = store.load_all_wolf().unwrap_or_else(|e| {
            warn!(?e, "failed to load werewolf games; starting empty");
            FoldHashMap::default()
        });
        Arc::new(Self {
            client,
            cfg,
            wolf_games: Mutex::new(wolf_games),
            bot_open_id: Mutex::new(None),
            seen_events: Mutex::new(FoldHashMap::default()),
            seen_actions: Mutex::new(FoldHashMap::default()),
            llm,
            store,
        })
    }

    pub fn cfg(&self) -> &Config {
        &self.cfg
    }
    pub fn set_bot_open_id(&self, id: String) {
        *self.bot_open_id.lock() = Some(id);
    }
    fn bot_open_id_clone(&self) -> Option<String> {
        self.bot_open_id.lock().clone()
    }

    pub(crate) fn is_duplicate_event(&self, event_id: &str) -> bool {
        if event_id.is_empty() {
            return false;
        }
        let mut seen = self.seen_events.lock();
        seen.retain(|_, t| t.elapsed() < Duration::from_secs(120));
        if seen.contains_key(event_id) {
            return true;
        }
        seen.insert(event_id.to_string(), Instant::now());
        false
    }

    pub(crate) fn is_duplicate_action(&self, action: &CardAction) -> bool {
        use std::hash::{BuildHasher, Hash, Hasher};
        let mut h = foldhash::fast::FixedState::default().build_hasher();
        action.open_id.hash(&mut h);
        sonic_rs::to_string(&action.value)
            .unwrap_or_default()
            .hash(&mut h);
        let key = h.finish();
        let mut seen = self.seen_actions.lock();
        seen.retain(|_, t| t.elapsed() < Duration::from_secs(10));
        if seen
            .get(&key)
            .is_some_and(|t| t.elapsed() < Duration::from_secs(3))
        {
            return true;
        }
        seen.insert(key, Instant::now());
        false
    }

    pub(crate) async fn send_user_only(
        &self,
        msg: &InboundMessage,
        card: &Value,
    ) -> Result<String> {
        if msg.chat_type == "p2p" {
            self.client
                .send_message("chat_id", &msg.chat_id, "interactive", card)
                .await
        } else {
            self.client
                .send_ephemeral_card(&msg.chat_id, &msg.sender_open_id, card)
                .await
        }
    }

    pub async fn handle_message(self: Arc<Self>, msg: InboundMessage) -> Result<()> {
        if self.is_duplicate_event(&msg.event_id) || msg.message_type != "text" {
            return Ok(());
        }
        if let Some(allowed) = &self.cfg.allowed_chat_id {
            if &msg.chat_id != allowed && msg.chat_type != "p2p" {
                return Ok(());
            }
        }
        let bot_id = self.bot_open_id_clone().unwrap_or_default();
        let Some(cmd) = parse_command(&msg.text, &msg.mentions, &bot_id, msg.chat_type == "p2p")
        else {
            return Ok(());
        };
        if let Err(e) = self.dispatch_command(cmd, &msg).await {
            let _ = self
                .send_user_only(
                    &msg,
                    &card(header("无法执行", "red"), vec![div_md(&e.to_string())]),
                )
                .await;
        }
        Ok(())
    }

    async fn dispatch_command(&self, cmd: Command, msg: &InboundMessage) -> Result<()> {
        match cmd {
            Command::WolfHelp => self.send_wolf_help(msg).await,
            Command::WolfJoin => {
                let name = self
                    .client
                    .user_name(&msg.sender_open_id)
                    .await
                    .unwrap_or_else(|_| "玩家".into());
                self.do_join(&msg.chat_id, &msg.sender_open_id, &name).await
            }
            Command::WolfLeave => self.do_leave(&msg.chat_id, &msg.sender_open_id).await,
            Command::WolfStart => self.do_start_wolf(&msg.chat_id).await,
            Command::WolfReset => self.do_reset(&msg.chat_id).await,
        }
    }

    async fn do_join(&self, chat_id: &str, open_id: &str, name: &str) -> Result<()> {
        {
            let mut games = self.wolf_games.lock();
            let game = games
                .entry(chat_id.to_string())
                .or_insert_with(|| WolfGame::new(chat_id.to_string()));
            game.add_player(open_id.to_string(), name.to_string())?;
            self.persist_wolf_locked(chat_id, game);
        }
        self.refresh_lobby(chat_id).await
    }

    async fn do_leave(&self, chat_id: &str, open_id: &str) -> Result<()> {
        {
            let mut games = self.wolf_games.lock();
            let game = games
                .get_mut(chat_id)
                .ok_or_else(|| anyhow!("当前没有狼人杀房间"))?;
            if !matches!(game.stage, Stage::Lobby | Stage::Ended) {
                return Err(anyhow!("狼人杀进行中，不能离开"));
            }
            let idx = game
                .find_player(open_id)
                .ok_or_else(|| anyhow!("你还没有加入房间"))?;
            game.players.remove(idx);
            self.persist_wolf_locked(chat_id, game);
        }
        self.refresh_lobby(chat_id).await
    }

    async fn fill_ai_lobby(&self, chat_id: &str) -> Result<()> {
        if self.llm.is_none() {
            return Err(anyhow!("机器人未配置 OPENAI_API_KEY，暂时无法使用 AI 补齐"));
        }
        {
            let mut games = self.wolf_games.lock();
            let game = games
                .entry(chat_id.to_string())
                .or_insert_with(|| WolfGame::new(chat_id.to_string()));
            if !matches!(game.stage, Stage::Lobby | Stage::Ended) {
                return Err(anyhow!("狼人杀已经开始"));
            }
            let mut next = game
                .players
                .iter()
                .filter_map(|p| p.open_id.strip_prefix("ai:"))
                .filter_map(|n| n.parse::<u32>().ok())
                .max()
                .unwrap_or(0)
                + 1;
            for _ in 0..ai_seats_needed(game.players.len()) {
                let persona = Persona::random();
                game.add_ai_player(
                    format!("ai:{next}"),
                    format!("{} #{}", persona.label(), game.players.len() + 1),
                    persona,
                )?;
                next += 1;
            }
            self.persist_wolf_locked(chat_id, game);
        }
        self.refresh_lobby(chat_id).await
    }

    pub(crate) async fn do_reset(&self, chat_id: &str) -> Result<()> {
        self.store.delete_wolf(chat_id)?;
        let game = WolfGame::new(chat_id.to_string());
        self.persist_wolf_locked(chat_id, &game);
        self.wolf_games.lock().insert(chat_id.to_string(), game);
        let _ = self
            .client
            .send_message(
                "chat_id",
                chat_id,
                "interactive",
                &card(
                    header("房间已重置", "wathet"),
                    vec![div_md("请在下方大厅卡片重新加入狼人杀。")],
                ),
            )
            .await;
        self.refresh_lobby(chat_id).await?;
        Ok(())
    }

    pub(crate) async fn do_start_wolf(&self, chat_id: &str) -> Result<()> {
        let (start_card, reveals) = {
            let mut games = self.wolf_games.lock();
            let game = games
                .get_mut(chat_id)
                .ok_or_else(|| anyhow!("当前没有玩家，先加入房间"))?;
            if !(9..=12).contains(&game.players.len()) {
                return Err(anyhow!(
                    "狼人杀需要 9-12 名玩家，当前 {}",
                    game.players.len()
                ));
            }
            if !matches!(game.stage, Stage::Lobby | Stage::Ended) {
                return Err(anyhow!("狼人杀正在进行中"));
            }
            game.start_game()?;
            let start = crate::werewolf::cards::build_game_start_card(game);
            let reveals = game
                .players
                .iter()
                .map(|p| {
                    (
                        p.open_id.clone(),
                        crate::werewolf::cards::build_role_reveal_card(game, p),
                        p.is_ai,
                    )
                })
                .collect::<Vec<_>>();
            self.persist_wolf_locked(chat_id, game);
            (start, reveals)
        };
        self.refresh_lobby(chat_id).await?;
        let _ = self
            .client
            .send_message("chat_id", chat_id, "interactive", &start_card)
            .await;
        for (id, card, is_ai) in reveals {
            if !is_ai {
                let _ = self.client.send_ephemeral_card(chat_id, &id, &card).await;
            }
        }
        self.advance_wolf(chat_id).await;
        Ok(())
    }

    pub(crate) async fn refresh_lobby(&self, chat_id: &str) -> Result<()> {
        let (value, old_id) = {
            let games = self.wolf_games.lock();
            let Some(game) = games.get(chat_id) else {
                return Ok(());
            };
            (build_lobby_card(game), game.lobby_msg_id.clone())
        };
        if let Some(id) = old_id {
            if self.client.update_card(&id, &value).await.is_ok() {
                return Ok(());
            }
        }
        let id = self
            .client
            .send_message("chat_id", chat_id, "interactive", &value)
            .await?;
        if let Some(game) = self.wolf_games.lock().get_mut(chat_id) {
            game.lobby_msg_id = Some(id);
            self.persist_wolf_locked(chat_id, game);
        }
        Ok(())
    }

    pub async fn handle_card_action(self: Arc<Self>, action: CardAction) -> Result<Value> {
        if self.is_duplicate_event(&action.event_id) || self.is_duplicate_action(&action) {
            return Ok(json!({}));
        }
        let Some(action_id) = action
            .value
            .get("action")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            return Ok(json!({}));
        };
        if action_id.starts_with("wolf_") {
            return self.handle_wolf_card_action(action, action_id).await;
        }
        let chat_id = action
            .value
            .get("chat_id")
            .and_then(Value::as_str)
            .unwrap_or(&action.open_chat_id)
            .to_string();
        let bot = self.clone();
        let oid = action.open_id.clone();
        tokio::spawn(async move {
            let result = match action_id.as_str() {
                "join_lobby" => {
                    let name = bot
                        .client
                        .user_name(&oid)
                        .await
                        .unwrap_or_else(|_| "玩家".into());
                    bot.do_join(&chat_id, &oid, &name).await
                }
                "leave_lobby" => bot.do_leave(&chat_id, &oid).await,
                "fill_ai_lobby" => bot.fill_ai_lobby(&chat_id).await,
                "start_wolf_lobby" => bot.do_start_wolf(&chat_id).await,
                "reset_lobby" => bot.do_reset(&chat_id).await,
                _ => Ok(()),
            };
            if let Err(e) = result {
                let _ = bot
                    .client
                    .send_ephemeral_card(
                        &chat_id,
                        &oid,
                        &card(header("无法执行", "red"), vec![div_md(&e.to_string())]),
                    )
                    .await;
            }
        });
        Ok(json!({}))
    }

    pub async fn handle_member_added(self: Arc<Self>, evt: MemberAdded) -> Result<()> {
        if self.is_duplicate_event(&evt.event_id) {
            return Ok(());
        }
        if self
            .cfg
            .allowed_chat_id
            .as_ref()
            .is_some_and(|id| id != &evt.chat_id)
        {
            return Ok(());
        }
        for user in evt.users {
            let c = card(
                header("🐺 欢迎来到狼人杀", "turquoise"),
                vec![
                    markdown(&format!("👋 **{}**，点击下方按钮加入房间。", user.name)),
                    button(
                        "加入狼人杀",
                        json!({"action":"join_lobby","chat_id":evt.chat_id}),
                        "primary",
                    ),
                ],
            );
            let _ = self
                .client
                .send_ephemeral_card(&evt.chat_id, &user.open_id, &c)
                .await;
        }
        Ok(())
    }

    pub async fn handle_bot_added(self: Arc<Self>, evt: BotAdded) -> Result<()> {
        if self.is_duplicate_event(&evt.event_id) {
            return Ok(());
        }
        if self
            .cfg
            .allowed_chat_id
            .as_ref()
            .is_some_and(|id| id != &evt.chat_id)
        {
            return Ok(());
        }
        let c = build_bot_added_welcome_card(self.llm.is_some(), &evt.operator_open_id);
        let _ = self
            .client
            .send_message("chat_id", &evt.chat_id, "interactive", &c)
            .await;
        {
            let mut games = self.wolf_games.lock();
            let game = games
                .entry(evt.chat_id.clone())
                .or_insert_with(|| WolfGame::new(evt.chat_id.clone()));
            self.persist_wolf_locked(&evt.chat_id, game);
        }
        self.refresh_lobby(&evt.chat_id).await?;
        Ok(())
    }

    pub async fn send_debug_lobby(&self, chat_id: &str, recipient: &str) -> Result<()> {
        self.refresh_lobby(chat_id).await?;
        let value = {
            let games = self.wolf_games.lock();
            games
                .get(chat_id)
                .map(build_lobby_card)
                .unwrap_or_else(|| build_lobby_card(&WolfGame::new(chat_id.to_string())))
        };
        self.client
            .send_ephemeral_card(chat_id, recipient, &value)
            .await?;
        Ok(())
    }

    pub async fn send_all_mocks(&self, chat_id: &str, recipient: &str) -> Result<()> {
        self.send_debug_lobby(chat_id, recipient).await
    }
}

fn parse_command(text: &str, mentions: &[Mention], bot_id: &str, p2p: bool) -> Option<Command> {
    let mut cleaned = text.to_string();
    let mut addressed = p2p;
    for mention in mentions {
        if mention.open_id == bot_id {
            addressed = true;
        }
        cleaned = cleaned.replace(&mention.key, "");
    }
    let trimmed = cleaned.trim();
    let body = if let Some(rest) = trimmed
        .strip_prefix("/wolf")
        .or_else(|| trimmed.strip_prefix("/狼人杀"))
        .or_else(|| trimmed.strip_prefix("/狼"))
    {
        rest.trim().to_string()
    } else if addressed {
        trimmed.to_string()
    } else {
        return None;
    };
    let mut parts = body.split_whitespace();
    let first = parts.next().unwrap_or("").to_lowercase();
    let sub = if matches!(first.as_str(), "wolf" | "狼" | "狼人" | "狼人杀") {
        parts.next().unwrap_or("help").to_lowercase()
    } else {
        first
    };
    Some(match sub.as_str() {
        "join" | "加入" => Command::WolfJoin,
        "leave" | "离开" => Command::WolfLeave,
        "start" | "begin" | "go" | "开始" => Command::WolfStart,
        "reset" | "重置" => Command::WolfReset,
        "help" | "帮助" | "?" | "" => Command::WolfHelp,
        _ => return None,
    })
}

fn build_lobby_card(game: &WolfGame) -> Value {
    let in_progress = !matches!(game.stage, Stage::Lobby | Stage::Ended);
    let subtitle = if in_progress {
        format!("狼人杀 · {} · 第 {} 天", game.stage.label(), game.day)
    } else {
        format!("已就座 {} 人 · 需要 9-12 人", game.players.len())
    };
    let mut elements = vec![markdown(if game.players.is_empty() {
        "🪑 房间空空如也，点击加入狼人杀。"
    } else {
        "**当前玩家**"
    })];
    if !game.players.is_empty() {
        let ids = game
            .players
            .iter()
            .filter(|p| !p.is_ai)
            .map(|p| p.open_id.clone())
            .collect::<Vec<_>>();
        if !ids.is_empty() {
            elements.push(person_list(&ids));
        }
        elements.push(markdown(
            &game
                .players
                .iter()
                .map(|p| {
                    format!(
                        "• {}{}",
                        if p.is_ai {
                            p.persona.map(|x| x.emoji()).unwrap_or("🤖")
                        } else {
                            "👤"
                        },
                        p.name
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    if !in_progress {
        let base = json!({"chat_id": game.chat_id});
        let mut roster = vec![button(
            "加入",
            merge(&base, &json!({"action":"join_lobby"})),
            "primary",
        )];
        roster.push(button(
            "AI 补齐人数",
            merge(&base, &json!({"action":"fill_ai_lobby"})),
            "default",
        ));
        if !game.players.is_empty() {
            roster.push(button(
                "离开",
                merge(&base, &json!({"action":"leave_lobby"})),
                "default",
            ));
        }
        roster.push(button(
            "重置",
            merge(&base, &json!({"action":"reset_lobby"})),
            "default",
        ));
        elements.push(actions(roster));
        if (9..=12).contains(&game.players.len()) {
            elements.push(button(
                "🐺 开始狼人杀",
                merge(&base, &json!({"action":"start_wolf_lobby"})),
                "primary",
            ));
        } else {
            elements.push(note_md(&format!(
                "还差 {} 人，或点击 AI 补齐人数",
                9usize.saturating_sub(game.players.len())
            )));
        }
    } else {
        elements.push(note_md("狼人杀进行中，结束后大厅会重新开放。"));
    }
    card(
        header_with_subtitle(
            "🐺 狼人杀 · 大厅",
            &subtitle,
            if in_progress { "wathet" } else { "turquoise" },
        ),
        elements,
    )
}

fn build_bot_added_welcome_card(ai_enabled: bool, inviter: &str) -> Value {
    let ai_note = if ai_enabled {
        "人数不足时可点击 **AI 补齐人数**。"
    } else {
        "配置 OPENAI_API_KEY 后可使用 AI 补齐人数。"
    };
    card(
        header_with_subtitle("🐺 夜局 · 狼人杀", "飞书群里的 AI 狼人杀", "turquoise"),
        vec![
            markdown(&if inviter.is_empty() {
                "狼人杀机器人已加入本群。".to_string()
            } else {
                format!("感谢 {} 把我拉进群。", at(inviter))
            }),
            markdown("点击大厅卡片加入，凑齐 9-12 人后开始。"),
            note(ai_note),
        ],
    )
}

fn merge(a: &Value, b: &Value) -> Value {
    let mut out = a.as_object().cloned().unwrap_or_default();
    if let Some(obj) = b.as_object() {
        out.extend(obj.clone());
    }
    Value::Object(out)
}

pub(crate) fn toast(content: &str) -> Value {
    json!({"toast":{"type":"info","content":content}})
}

fn ai_seats_needed(player_count: usize) -> usize {
    9usize.saturating_sub(player_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_fill_targets_minimum_nine_players() {
        assert_eq!(ai_seats_needed(0), 9);
        assert_eq!(ai_seats_needed(4), 5);
        assert_eq!(ai_seats_needed(9), 0);
        assert_eq!(ai_seats_needed(12), 0);
    }

    #[test]
    fn lobby_contains_one_click_ai_fill_action() {
        let game = WolfGame::new("oc_test".into());
        let encoded = sonic_rs::to_string(&build_lobby_card(&game)).unwrap();
        assert!(encoded.contains("AI 补齐人数"));
        assert!(encoded.contains("fill_ai_lobby"));
    }

    #[test]
    fn command_parser_only_accepts_werewolf_namespace() {
        assert_eq!(
            parse_command("/wolf join", &[], "", false),
            Some(Command::WolfJoin)
        );
        assert_eq!(parse_command("/unknown join", &[], "", false), None);
    }
}
