use serde::{Deserialize, Serialize};

/// AI 玩家在狼人杀中的策略风格。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Persona {
    LooseAggressive,
    TightAggressive,
    LooseWeak,
    TightWeak,
    Maniac,
}

impl Persona {
    pub fn label(self) -> &'static str {
        match self {
            Self::LooseAggressive => "莽哥",
            Self::TightAggressive => "老炮",
            Self::LooseWeak => "跟注站",
            Self::TightWeak => "老抠",
            Self::Maniac => "头铁",
        }
    }

    pub fn werewolf_description(self) -> &'static str {
        match self {
            Self::LooseAggressive => {
                "**悍跳激进流**。第一时间表态、抢警、爆身份，靠话术密度和节奏压迫对手。"
            }
            Self::TightAggressive => {
                "**逻辑推理流**。回顾每轮信息，结论基于清晰的因果链条，不被气势带偏。"
            }
            Self::LooseWeak => "**节奏控场流**。梳理观点并主导讨论方向，关键票带头表态。",
            Self::TightWeak => "**静水深流**。话少但精准，汇总信息后给出关键结论。",
            Self::Maniac => "**反水诡道流**。伪装阵营、制造混乱，反直觉但每一步都经过算计。",
        }
    }

    pub fn emoji(self) -> &'static str {
        match self {
            Self::LooseAggressive => "🐺",
            Self::TightAggressive => "🦈",
            Self::LooseWeak => "🐟",
            Self::TightWeak => "🪨",
            Self::Maniac => "🤪",
        }
    }

    pub fn random() -> Self {
        const ALL: [Persona; 5] = [
            Persona::LooseAggressive,
            Persona::TightAggressive,
            Persona::LooseWeak,
            Persona::TightWeak,
            Persona::Maniac,
        ];
        ALL[fastrand::usize(0..ALL.len())]
    }
}
