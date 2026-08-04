# discord.ind — The Discord Bot Library for Indent

A standalone Discord library (like discord.py) providing REST API, WebSocket
Gateway, command routing, event system, puzzle/cog loader, a discord.py-style
`ctx` system, and clean block-style helpers.  Import it and write your bot.

> **Note**: Aether `.ath` files are the same language as Indent `.ind` — you can
> just rename them.  `discord` is a single package available as `discord.ind`.
>
> **Important**: the native Indent runtime passes function arguments *by value*,
> so every function that modifies the bot **returns the modified bot** and you
> **must reassign** the result:
> ```indent
> bot is addCmd bot "ping" pingCmd empty
> bot is On bot "ready" "onReady"
> bot is LoadPuzzles bot "puzzles"
> ```

```indent
get NewBot  from discord
get start   from discord
get addCmd  from discord
get on      from discord
get sendMsg from discord

var bot dynamic = NewBot "YOUR_TOKEN" "!"

fun pingCmd args
    sendMsg bot "🏓 Pong!"
bot is addCmd bot "ping" pingCmd empty

fun onReady bot data
    sendMsg bot "✅ Online!"
bot is on bot "ready" onReady

start bot
```

---

## 0. The Absolute Easiest Bot (2 lines)

Put `DISCORD_TOKEN="..."` in a `.glo` file, put any `command "name" "desc" "reply"`
lines in a `puzzles/` folder, and this is your entire bot:

```indent
get QuickStart from discord
QuickStart
```

`QuickStart` reads the token from the environment, auto-loads every puzzle in
`puzzles/`, and connects.  Add commands by dropping files into `puzzles/`:

```indent
#! puzzles/fun.ind
command "ping" "Check latency" "🏓 Pong!"
command "roll" "Roll a die" "🎲 You rolled a 6!"
```

No token/source juggling, no boilerplate.  For a bot with real code (handlers),
see the Ctx System in section 4b — handlers are just `fun name ctx args`.

---

## Quick Reference

| Task | Function | Signature |
|---|---|---|
| Easiest bot (env + puzzles) | `QuickStart` ★ | `()` |
| Create bot from env | `MakeBot` / `BotFromEnv` ★ | `→ bot dict` |
| Create bot | `NewBot` | `token, prefix → bot dict` |
| Start bot | `start` (alias: `Run`) | `bot` |
| Register command | `addCmd` ★ | `bot, name, handler, [argNames]` |
| Register command (no args) | `add` | `bot, name, handler` |
| Register handler cmd | `BotHandler` ★ | `bot, name, desc, handlerName` |
| Register slash command | `AddSlash` / `SlashWithUser` ★ | `bot, name, desc, handler, opts` |
| Sync slash commands | `SyncSlash` | `bot` |
| Register event | `on` | `bot, event, handler` |
| Send message reply | `sendMsg` ★ | `bot, message` |
| Kick user | `kick` | `bot, user, reason` |
| Ban user | `ban` | `bot, user, reason` |
| DM user | `dm` | `bot, user, message` |
| Add role | `addRole` | `bot, user, roleId` |
| Remove role | `removeRole` | `bot, user, roleId` |
| Load puzzle/cog dir | `load` | `bot, dir` |

★ = recommended over older alternatives

---

## 1. Creating & Starting a Bot

### `NewBot(token, prefix) → bot`

Creates a bot dict.  Everything lives on this dict — commands, handlers,
context, user info.

```indent
get NewBot from discord
var bot dynamic = NewBot "YOUR_BOT_TOKEN" "!"
```

### `start(bot)` / `Run(bot)`

Connects to the Discord Gateway, sends identify, starts the heartbeat
loop, and begins processing events.  Blocks forever (or until `stop`).

```indent
get start from discord
start bot
```

### `QuickBot(token, prefix) → bot`

Shorter alias for `NewBot`.  Same behaviour.

### `BotFromEnv() → bot`

Reads `DISCORD_TOKEN` (or `BOT_TOKEN`) and `DISCORD_PREFIX` from
environment / `.glo` config.  Exits if no token found.

---

## 2. Commands

### `addCmd(bot, name, handler, argNames) → bot` ★ RECOMMENDED

Registers a prefix command WITH argument metadata.  The package
automatically validates required args before the handler runs.

```indent
get addCmd from discord

fun kickCmd args
    var user    string = args["1"]       # guaranteed non-empty
    var reason  string = args["2"]       # optional — may be empty
    if reason == empty
        reason is "No reason given"
    kick bot user reason
    sendMsg bot "✅ Kicked <@" + user + ">"

bot is addCmd bot "kick" kickCmd ["user"]
```

If a user types `!kick` without a user, the package replies:
> ❌ Missing argument 1: user

The command handler never runs.  Handler code stays clean — no manual
`if user == empty` checks needed.

**`argNames`**: a list of names for required args.  Only list args that
are truly required.  Optional args (like `reason` above) get their
defaults inside the handler body.

### `add(bot, name, handler) → bot`

Registers a command WITHOUT arg validation.  Use for commands with
no required arguments or when you want full manual control.

```indent
get add from discord

fun pingCmd args
    sendMsg bot "🏓 Pong!"
bot is add bot "ping" pingCmd

fun infoCmd args
    sendMsg bot "**MyBot** v1.0"
bot is add bot "info" infoCmd
```

> **Important**: `add` and `addCmd` return the modified bot.
> You MUST reassign: `bot is add bot "name" handler`

### Built-in commands

`ping` and `help` are handled automatically by the package — you don't
need to register them.  `ping` measures Discord API latency; `help`
lists all registered commands.

### Command handler signature

```indent
fun myHandler args
    # args is a dict: {"1": "first arg", "2": "second", ...}
    # Keys "1"–"9" always exist (empty if not provided)
    var first  string = args["1"]
    var second string = args["2"]
```

### Simple reply commands

For one-liner commands, use `SimpleCommand`:

```indent
get SimpleCommand from discord
SimpleCommand bot "hello" "Greet someone" "Hello there! 👋"
```

---

## 3. Events

### `on(bot, event, handler) → bot`

Registers an event handler.  Returns bot — reassign.

```indent
get on from discord

fun onReady bot data
    sendMsg bot "✅ Bot is online!"

fun onMessage bot msg
    # fires on every message (commands still work)

bot is on bot "ready"   onReady
bot is on bot "message" onMessage
```

**Supported events**: `ready`, `message`, `guild_join`, `member_join`,
`member_leave`

### Event handler signatures

| Event | Handler signature |
|---|---|
| `ready` | `fun handler bot data` — data is the READY payload |
| `message` | `fun handler bot msg` — msg is the message object |
| `guild_join` | `fun handler bot data` — data is the guild object |
| `member_join` | `fun handler bot data` — data is the member object |
| `member_leave` | `fun handler bot data` — data is the member object |

---

## 4. Clean Block API (no token/source noise)

These functions auto-extract `token` and `guild_id` from the bot's
context.  Call them from inside command handlers — no manual
token/source wrangling needed.

### `sendMsg(bot, message)`

Replies to the current channel or interaction.

```indent
sendMsg bot "Hello, World!"
```

### `kick(bot, user, reason)`

Kicks a user from the guild.

```indent
kick bot userId "Spamming"
```

### `ban(bot, user, reason)`

Bans a user from the guild.

```indent
ban bot userId "Breaking rules"
```

### `dm(bot, user, message)`

Sends a direct message to a user.

```indent
dm bot userId "Hey there!"
```

### `addRole(bot, user, roleId)`

Adds a role to a user.

```indent
addRole bot userId "1234567890"
```

### `removeRole(bot, user, roleId)`

Removes a role from a user.

```indent
removeRole bot userId "1234567890"
```

---

## 4b. Ctx System (discord.py-style) ★ RECOMMENDED

The most powerful way to write commands: handlers receive a `ctx` object just
like discord.py, plus the raw argument list.  Register them with `BotHandler`
(named function) and dispatch happens automatically through the `command` event.

### `BotHandler(bot, name, desc, handlerName) → bot`

Registers a handler-based command.  `handlerName` is the *string* name of a
function in your script.  Returns bot — **reassign**.

```indent
get BotHandler from discord
get CtxSend from discord
get CtxGuildId from discord
get CtxReply from discord
get Ban from discord

fun handleBan ctx args
    var userId string = args[0]
    var reason string = "Banned by Angela"
    var guildId string = CtxGuildId ctx
    Do:
        Ban ctx.token guildId userId reason
        CtxSend ctx "✅ Banned `" + userId + "`"
    Catch as err:
        CtxSend ctx "❌ Ban failed: " + err

bot is BotHandler bot "ban" "Ban a user by ID" "handleBan"
```

### Handler signature

```indent
fun myHandler ctx args
    # ctx.author       — the user who ran the command
    # ctx.channel_id   — channel ID
    # ctx.guild_id     — guild ID (empty in DMs)
    # ctx.token        — bot token
    # ctx.source       — raw message or interaction
    # ctx.cmd          — command name
    # ctx.args         — list of arguments
    # ctx.bot          — the bot object
    # args[0], args[1], ... — positional arguments
```

### Ctx helpers

| Function | What it does |
|---|---|
| `CtxSend ctx content` | sends a message to the command's channel |
| `CtxReply ctx content` | replies to the message / interaction |
| `CtxEmbed ctx embed` | sends an embed to the channel |
| `CtxEphemeral ctx content` | ephemeral reply (user-only) to an interaction |
| `CtxEdit ctx content` | edits the original interaction response |
| `CtxUserId ctx` | the author's user ID |
| `CtxUserName ctx` | the author's username |
| `CtxGuildId ctx` | the guild ID (empty in DMs) |
| `CtxChannelId ctx` | the channel ID |
| `CtxIsInteraction ctx` | true if the source is an interaction |

```indent
get CtxSend from discord
fun greet ctx args
    var name string = ctx.author.username
    CtxSend ctx "Hello, " + name + "! 👋"
bot is BotHandler bot "greet" "Greet someone" "greet"
```

---

## 5. Embeds

### `QuickEmbed(title, description, color) → embed`

Creates an embed dict.

```indent
get QuickEmbed from discord
var embed dynamic = QuickEmbed "Title" "Description" 0x3498DB
```

### `ReplyWithEmbed(bot, embed)`

Sends an embed reply.  (Uses `bot._ctx` for source.)

```indent
get ReplyWithEmbed from discord
ReplyWithEmbed bot embed
```

---

## 6. REST API (Low-Level)

All return the HTTP response wrapper: `{ok, status, body}` where `body`
is the raw JSON string — use `json_loads` to parse.

| Function | HTTP | Signature |
|---|---|---|
| `Get` | GET | `path, token` |
| `Post` | POST | `path, token, body` |
| `Put` | PUT | `path, token, body` |
| `Patch` | PATCH | `path, token, body` |
| `Delete` | DELETE | `path, token` |
| `Send` | POST | `token, channelId, content` |
| `Reply` | POST | `token, channelId, messageId, content` |
| `SendEmbed` | POST | `token, channelId, embed` |
| `Edit` | PATCH | `token, channelId, messageId, content` |
| `DeleteMsg` | DELETE | `token, channelId, messageId` |
| `React` | PUT | `token, channelId, messageId, emoji` |
| `SendDM` | POST | `token, userId, content` |
| `Kick` | DELETE | `token, guildId, userId, reason` |
| `Ban` | PUT | `token, guildId, userId, reason` |
| `Unban` | DELETE | `token, guildId, userId` |
| `Timeout` | PATCH | `token, guildId, userId, seconds` |
| `AddRole` | PUT | `token, guildId, userId, roleId` |
| `RemoveRole` | DELETE | `token, guildId, userId, roleId` |
| `GetUser` | GET | `token, userId` |
| `GetGuild` | GET | `token, guildId` |
| `GetChannel` | GET | `token, channelId` |

---

## 7. Puzzle System (Cog-like command groups)

Puzzles are self-contained `.ind` files in a `puzzles/` directory.
Each file can define multiple commands — like Discord.py cogs.

### Puzzle file format

```indent
#! puzzles/fun.ind
get sendMsg from discord
get add from discord

fun rollCmd args
    # handler code...
bot is add bot "roll" rollCmd

fun flipCmd args
    # handler code...
bot is add bot "flip" flipCmd
```

### Loading puzzles

```indent
get load from discord
load bot "puzzles"
```

Puzzles load all `.ind` files from the directory.  Each puzzle
registers its own commands using `add`/`addCmd` and imports what it
needs from `discord`.

---

## 8. Complete Working Example

```indent
#! ============================================================
#! MyBot — Complete discord.ind example
#! ============================================================
#!
#! Prerequisites: a config.glo file with BOT_TOKEN and BOT_PREFIX
#!
#! Run:  indent run mybot.ind
#! ============================================================

#! ---- 1. Import config --------------------------------------
get BOT_TOKEN  from config
get BOT_PREFIX from config

#! ---- 2. Import discord package -----------------------------
get NewBot  from discord
get start   from discord
get addCmd  from discord
get on      from discord
get sendMsg from discord
get kick    from discord
get ban     from discord
get dm      from discord
get load    from discord

#! ---- 3. Create the bot -------------------------------------
var bot dynamic = NewBot BOT_TOKEN BOT_PREFIX

#! ---- 4. Command handlers -----------------------------------
fun pingCmd args
    sendMsg bot "🏓 Pong!"

fun greetCmd args
    var name string = args["1"]
    if name == empty
        name is "World"
    sendMsg bot "Hello, " + name + "! 👋"

fun kickCmd args
    var user   string = args["1"]
    var reason string = args["2"]
    if reason == empty
        reason is "No reason given"
    kick bot user reason
    sendMsg bot "👢 Kicked <@" + user + ">"

fun banCmd args
    var user   string = args["1"]
    var reason string = args["2"]
    if reason == empty
        reason is "No reason given"
    ban bot user reason
    sendMsg bot "🔨 Banned <@" + user + ">"

#! ---- 5. Register commands ----------------------------------
#! Use addCmd with arg names for auto-validation
bot is addCmd bot "ping"  pingCmd  empty          # no required args
bot is addCmd bot "greet" greetCmd ["name"]       # name is optional? keep add
bot is addCmd bot "kick"  kickCmd  ["user"]
bot is addCmd bot "ban"   banCmd   ["user"]

#! ---- 6. Event handlers -------------------------------------
fun onReady bot data
    sendMsg bot "✅ Bot is online!"

bot is on bot "ready" onReady

#! ---- 7. Load puzzles ---------------------------------------
load bot "puzzles"

#! ---- 8. Start! ---------------------------------------------
start bot
```

---

## 9. Best Practices

1. **Use `addCmd` with arg names** — automatic helpful error messages,
   cleaner handler code.

2. **Reassign `bot`** — `add`, `addCmd`, and `on` return the modified bot:
   ```indent
   bot is add bot "ping" pingCmd
   ```

3. **Use `sendMsg`, not `say`** — `say` is an Indent keyword and
   `say bot msg` won't work at the statement level.  `sendMsg` is the
   recommended name.

4. **Pre-compute strings** in function arguments — Indent's parser
   handles `+` in function args differently:
   ```indent
   #! ❌ May fail:
   ReplyTo token msg "Error: " + err
   #! ✅ Safe:
   var msg string = "Error: " + err
   ReplyTo token msg msg
   ```

5. **Import only what you need** — keeps the bot namespace clean:
   ```indent
   get NewBot  from discord
   get addCmd  from discord
   get sendMsg from discord
   ```

6. **Error resilience is built in** — command errors are caught and
   reported in Discord.  Event handler errors log to console.  The
   bot never crashes from a bad command.

---

## 10. Error Messages Reference

| Trigger | Message |
|---|---|
| `!kick` (no user) | `❌ Missing argument 1: user` |
| `!xyz` (unknown cmd) | `Unknown: \`xyz\`` |
| Handler throws | `❌ Command error: <details>` |
| Bad syntax in code | `❌ Error: <details>` |
| Gateway disconnects | `📡 Gateway closed connection` (console) |
| Event handler fails | `⚠️  ready/message event error: <details>` (console) |

---

## 11. Capabilities Reference (Everything Else)

The package also ships a large set of ready-made helpers.  Import what you use.

### 💬 Slash commands

| Function | Purpose |
|---|---|
| `AddSlash bot name desc handler opts` | register a slash command (returns bot) |
| `SlashWithUser / String / Int / Channel / Role` | one-line slash with a single option |
| `SyncSlash bot` | push local slash commands to Discord |
| `StringOption / IntOption / BoolOption / UserOption / ...` | option builders |

```indent
get SlashWithUser from discord
get SyncSlash from discord
fun slashKick ctx args
    var target string = args["user"]
    CtxSend ctx "Kicking " + target
bot is SlashWithUser bot "kick" "Kick a user" "slashKick"
SyncSlash bot        # call before Run
Run bot
```

### 🌟 Presence & status

`SetStatus bot "online"` · `SetPlaying bot "with Angela"` · `SetWatching bot "x"` ·
`SetListening bot "x"` · `SetCompeting bot "x"` — all send a gateway presence update.

### 🎨 Embeds

`BuildEmbed t d c` · `HexColor "3498DB"` (hex→int, since `0x` literals aren't
supported) · `AddEmbedField emb n v inline` · `SetEmbedFooter / Image / Thumbnail /
Author / Timestamp` — embed builders return the embed, so chain with reassignment:
`emb is AddEmbedField emb "A" "1" true`.

### 🔘 Message components

`Button style label customId` · `PrimaryButton / SecondaryButton / SuccessButton /
DangerButton label customId` · `LinkButton label url` · `ActionRow components` ·
`SelectMenu customId options` · `SelectOption label value desc` ·
`SendWithComponents token channel content rows` · `ReplyWithComponents bot content rows` ·
`CtxComponents ctx content rows` · `CtxEphemeralComponents ctx content rows`.

### 🧱 Channels

`CreateChannel token guild name type` · `DeleteChannel` · `EditChannel` ·
`GetChannels` · `CreateThread token channel name`.

### 💬 Messages

`GetMessages` · `GetMessage` · `EditMessage` · `Pin` · `Unpin` · `DeleteMany` ·
`SendTo token channel content`.

### 👥 Members & roles

`GetMember` · `ListMembers` · `SetNickname` · `CreateRole` · `DeleteRole` ·
`GetRoles` · `ListBans` · plus `Kick` / `Ban` / `Unban` / `Timeout` / `AddRole` /
`RemoveRole`.

### 🏛️ Guilds

`CreateInvite token channel maxUses` · `GetInvites` · `CreateWebhook` ·
`GetWebhooks`.

### 👍 Reactions

`AddReaction` · `RemoveReaction` · `RemoveAllReactions`.

### 🧵 Interaction followups

`InteractionFollowup token interaction content` · `CtxFollowup ctx content` —
send a follow-up message after an interaction reply.

### 🔉 Voice

`MoveMember token guild user channel` · `DisconnectMember token guild user`.

### 😀 Emoji & stickers

`CreateEmoji token guild name imageB64 roles` · `GetEmojis` · `GetStickers` ·
`CreateSticker token guild name desc tags imageDataUri`.

### 🔗 Webhooks

`ExecuteWebhook webhookId webhookToken content` — send through a webhook
(no bot token needed).

### ⚙️ Slash command management

`GetSlashCommands token` · `EditSlash token commandId payload` ·
`DeleteSlash token commandId`.

### 🤖 Bot info & invite

`GetBotInfo token` · `BotInvite token permissions` — builds a bot invite link.

### 🧭 More ctx helpers

`CtxReplyEmbed ctx embed` · `CtxAuthorMention ctx` · `CtxChannelMention ctx`.

### ⏱️ Timestamp & duration

`DiscordTimestamp ts fmt` — `<t:...:R>` markup (`fmt`: R/t/T/d/D/f/F) ·
`ParseDuration "2h30m"` — human duration → seconds.

### 📊 Polls

`CreatePoll token channel question options hours` — build a poll.

### 🔐 Permissions & threads

`SetChannelPermissions token channel overwrite allow deny type` ·
`AddThreadMember` · `RemoveThreadMember`.

### 📅 Scheduled events

`CreateEvent token guild name desc channel startIso` — schedule a voice event.
