# 夜局 · lark-arena

飞书群聊里的 AI 狼人杀机器人。支持 9-12 人板型、私密身份卡、夜间技能、上警、警徽、遗言、猎人/狼王开枪，以及 OpenAI-compatible LLM 驱动的 AI 玩家。

## 功能

- 9 人：3 狼 / 预言家 / 女巫 / 猎人 / 3 村民，不上警
- 10 人：2 狼 + 狼王 / 预言家 / 女巫 / 猎人 / 守卫 / 3 村民
- 11 人：2 狼 + 狼王 / 预言家 / 女巫 / 猎人 / 守卫 / 4 村民
- 12 人：3 狼 + 狼王 / 预言家 / 女巫 / 猎人 / 守卫 / 4 村民
- 身份、夜间技能和投票通过 ephemeral 卡片发送，仅本人可见
- 采用暗牌屠边：出局时不公开身份，结算时统一揭晓；屠民或屠神即狼人胜利
- 首夜先竞选警长再公布死讯；警长和放逐首轮平票均进入 PK 发言与复投
- 警上候选人发言后可以退水，退水者不能参加警长投票
- 狼队击杀目标必须一致，否则视为空刀；狼人白天可以自爆结束当天
- 守卫可以空守；首夜死亡者有遗言，之后的夜间死亡者没有遗言
- 房间和对局状态使用 redb 持久化
- 大厅支持逐个加入/移除 AI，也可一键用 AI 补齐到 9 人
- AI 支持 OpenAI、DeepSeek、Doubao 等 OpenAI-compatible endpoint

## 配置

复制 `.env.example` 为 `.env`，至少填写：

```dotenv
FEISHU_APP_ID=cli_xxx
FEISHU_APP_SECRET=xxx
```

启用 AI 玩家还需要：

```dotenv
OPENAI_API_KEY=sk-xxx
OPENAI_BASE_URL=https://api.openai.com/v1
OPENAI_MODEL=gpt-4.1-mini
OPENAI_REASONING_EFFORT=xhigh
```

`OPENAI_REASONING_EFFORT` 控制模型思考强度，可选值为 `none`、`minimal`、`low`、`medium`、`high`、`xhigh`，默认 `xhigh`。具体档位是否生效取决于所使用的模型和 OpenAI-compatible 服务商。

可选配置：

```dotenv
FEISHU_VERIFICATION_TOKEN=
ALLOWED_CHAT_ID=
BIND_ADDR=0.0.0.0:3000
LARK_ARENA_DB_PATH=./lark-arena.redb
```

## 飞书事件

在飞书开放平台配置以下订阅和回调：

| 类型 | 路径 / 事件 |
| --- | --- |
| Event 回调 | `/webhook/event` |
| Card 回调 | `/webhook/card` |
| 消息 | `im.message.receive_v1` |
| 用户进群 | `im.chat.member.user.added_v1` |
| 机器人进群 | `im.chat.member.bot.added_v1` |

## 命令

可使用 `/wolf <命令>`，或在群里 `@机器人 <命令>`：

| 命令 | 作用 |
| --- | --- |
| `join` / `加入` | 加入狼人杀房间 |
| `leave` / `离开` | 离开房间 |
| `start` / `开始` | 9-12 人时开局 |
| `reset` / `重置` | 重置房间 |
| `help` / `帮助` | 查看帮助 |

大厅卡片提供同等操作：点击 **加入 AI** 可逐个增加到 12 人，点击 **移除 AI**
会移除最后一名 AI；人数不足时点击 **AI 补齐人数** 会一次补足到 9 人。

## 开发

```bash
cargo check --locked
cargo test --locked
cargo run --release
```

容器构建：

```bash
docker build -t lark-arena .
```

项目要求 Rust 1.89+，使用 Rust 2024 edition。

## 目录

```text
src/
├── bot.rs              狼人杀大厅、命令与卡片入口
├── config.rs           环境配置
├── feishu/             飞书事件、API 客户端与卡片组件
├── llm.rs              OpenAI-compatible JSON 对话客户端
├── persona.rs          AI 策略风格
├── server.rs           Axum webhook 服务
├── storage.rs          redb 狼人杀存档
└── werewolf/           状态机、卡片、AI 决策与推进循环
```

## License

GPL-3.0
