//! 狼人杀模式（Werewolf）。
//!
//! 模块划分：
//! - `game`：状态机、角色、阶段、胜负判定（纯逻辑，无 IO）
//! - `cards`：飞书卡片渲染
//! - `llm`：每个角色的 AI 决策 prompt
//! - `handlers`：Bot 的狼人杀回调与 AI 推进循环（impl Bot）

pub mod cards;
pub mod game;
pub mod handlers;
pub mod llm;

pub use game::WolfGame;
