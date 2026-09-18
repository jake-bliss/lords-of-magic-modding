# Multiplayer

What *Lords of Magic: Special Edition* multiplayer actually is, recovered from `lomse.exe`, from
the shipped GameScript corpus, and from the files installed on this machine. No part of this was
established by running the game; nothing here was tested against a second machine.

Every claim carries an evidence label, using the repo's classes:

- **Observed** — reproduced locally or read directly out of a file, with the address or path cited.
- **Documented** — stated in original or community documentation.
- **Inferred** — strongly suggested by the evidence, not proven.
- **Unknown** — open, and named as open rather than guessed.

The distinction that matters most in this document is between what was read out of *this* binary and
what is merely true of DirectPlay in general. General DirectPlay behaviour is labelled
**Documented** everywhere it appears and is never used to support a claim about this game.

Reproduce the structural parts with:

```
cd spikes/asset-viewer
cargo run --release --example multiplayer_survey -- <path to lomse.exe> [--imports]
cargo run --release --example disasm -- <path to lomse.exe> <hex address> [count]
```

The executable inspected throughout is the Steam/GOG `lomse.exe`, 1,535,488 bytes, 32-bit PE, image
base `0x00400000`, four sections. The GameScript corpus inspected is the **vanilla** `gs.mpq`
(1,795,987 bytes, from `English/_vanilla_backup/`), because the unofficial 3.02 patch replaces only
`gs.mpq` and `pic.mpq` and I wanted Sierra's script, not the community's.

---

## Summary

**Observed.** The game is a **lockstep, deterministic, peer-to-peer simulation**. Every peer runs
the whole world; only orders and a small set of state pokes cross the wire; the engine continuously
compares six independent checksums between peers and reports "Divergence" when they disagree. There
is no authoritative host, no dedicated server binary, and no state-streaming design.

**Consequence for the homelab question: a homelab cannot fix "touchy".** The dominant failure mode
is *desync*, which is a determinism problem, not a networking problem. A homelab can make one
genuine contribution — an always-on host peer and a layer-2 VPN so DirectPlay enumeration works —
and that is a real improvement to *reachability*, but reachability is not what is broken. The
detailed reasoning is in [question 4](#4-what-a-homelab-could-and-could-not-do).

---

## 1. Lockstep-deterministic, or host-authoritative?

**Lockstep-deterministic. Observed, from four independent directions.**

### 1a. The engine compares six checksums between peers and calls disagreement "Divergence"

**Observed in a local binary.** `lomse.exe` contains this family of format strings (file offsets
given; add `0x555000 - 0x153200` for the virtual address):

| Offset | String |
| ---: | --- |
| `0x15b1e0` | `Value for computer #%d=%d, value for computer #%d=%d` |
| `0x15b216` | `Divergence occurred after processing message ID #%d (%s), which occurred at Tick #%d` |
| `0x15b26c` | `Divergence was detected at Tick#%d` |
| `0x15b294` | `* Computer #%d (%s) has a different 'EXE' version than computer #%d (%s) *` |
| `0x15b2e4` | `* Computer #%d (%s) now has a different 'PlayAnimation Count' than computer #%d (%s) *` |
| `0x15b340` | `* Computer #%d (%s) now has a different 'Random-Seed' than computer #%d (%s) *` |
| `0x15b394` | `* Computer #%d (%s) has different 'IMP' files than computer #%d (%s) *` |
| `0x15b3e0` | `* Computer #%d (%s) has different 'GS' files than computer #%d (%s) *` |
| `0x15b428` | `* Computer #%d (%s) now has different 'Player-Stats' than computer #%d (%s) *` |
| `0x15b478` | `Error: Checksum queue is full for ID #%d...` |

The reporting code is at `0x004b4fc0`–`0x004b5260`; the `'Player-Stats'`, `'GS files'`,
`'IMP files'`, `'Random-Seed'`, `'PlayAnimation Count'` and `'EXE' version` strings are pushed at
`0x004b5010`, `0x004b507d`/`0x004b5095`, `0x004b50f9`, `0x004b516a`, `0x004b51db` and `0x004b5249`
respectively.

You do not build a per-tick, per-message-ID cross-peer comparison of a random seed and a
simulation-state checksum unless each peer is independently simulating. **Inferred, but only just:**
this is the definition of a lockstep desync detector.

The `'PlayAnimation Count'` divergence class is the single most informative line in the binary.
**Inferred:** an animation playback counter participates in the shared determinism, so a machine
that plays one more or one fewer animation frame than its peer diverges. That is a *rendering*
quantity feeding a *simulation* comparison, and it is exactly the shape of a bug that presents as
"multiplayer is really touchy".

### 1b. The shipped script hands the engine a checksum over the entire simulation state

**Observed in the corpus.** `gs/network.gs` in vanilla `gs.mpq` registers, via the
`setchecksumproc` operator, a procedure that sums — for every player `0..7` that is a real computer
or AI — every army's location and facing, and for every unit its type, health, hit points, movement
points, champion type, experience points, unique ID and all seven unit-modifier words, plus that
player's current gold, food and crystals.

The same file registers, via `setstatlogproc`, a procedure that writes *the same state* to the
network log through the `netlog` operator, army by army and unit by unit, so that two peers' logs
can be diffed after a divergence.

**Observed in a local binary.** `setchecksumproc` (`0x004b56e0`) stores the procedure object into
`0x005843f0`/`0x005843f4`, and the only reader of those two words is `0x004b4c81`/`0x004b4c97`,
inside the checksum module at `0x004b4b60`–`0x004b5300`. So the script-supplied whole-state checksum
is what the network layer compares.

A host-authoritative design has nothing to checksum: the state is identical because one peer owns
it.

### 1c. What crosses the wire is orders, not state

**Observed in a local binary.** A contiguous block of message-type names sits at file offsets
`0x159c1c`–`0x15a1f0`. It is around a hundred entries. Representative ones, with the reading they
force:

| Message names | What this means |
| --- | --- |
| `MOVE_ARMY`, `STOP_ARMY`, `FIND_MOVE`, `GET_ARMY_OUT`, `MOVE_ARMY_STEALTHILY`, `UNPAUSE_ARMY`, `ORDERS`, `BATCH_ORDERS` | player intent, replayed on every peer |
| `BUY_UNIT`, `ADD_BUILDING`, `ADD_CAPITOL`, `TRANSFORM_UNIT`, `DISMISS_UNIT`, `ADD_SPELL`, `WIELD_ARTIFACT` | economic and roster actions as commands |
| `START_COMBAT`, `END_COMBAT`, `ADD_MISSILE`, `ARMY_ANIM`, `ARMY_KILLUNIT`, `LEAVE_COMBAT` | real-time combat as an event stream |
| `TICK_TIME`, `PULSE`, `END_TURN`, `READY`, `TIME_ESTIMATE`, `GAME_STARTED`, `START_GAME` | the shared clock and the turn barrier |
| `SCRIPTCALLBACK`, `CHAMPION_BRAIN_NOTIFICATION`, `EVENT_ALARM_NOTIFICATION`, `SET_SPRITE_PROC` | **GameScript execution itself is replicated** |
| `CHECKSUM` | the comparison from 1a, as its own message type |
| `SET_UNIT_DATA`, `SET_ARMY_DATA`, `SET_PLAYER_DATA`, `SET_CITY_DATA`, `SET_BUILDING_DATA`, `SET_TERRAIN`, `SET_ELEVATION`, `PAINT_TERRAIN` | targeted state pokes |
| `REQUEST_SCENARIO`, `XFER`, `XFER_PROGRESS`, `RESYNC_START`, `RESYNC_REQUEST` | bulk transfer, used at setup and for recovery |

**Observed.** So it is not purely lockstep: there is a state-poke vocabulary alongside the command
vocabulary, and a resync path. **Inferred:** the pokes exist for quantities the designers did not
trust to reproduce, and the resync path is a recovery mechanism rather than the normal data flow —
because if state were shipped normally, `CHECKSUM` and the six divergence classes would have nothing
to do.

The presence of `SCRIPTCALLBACK` is the reason `'GS' files` is a divergence class. **Inferred:** the
GameScript interpreter runs on every peer, so the entire script corpus is part of the deterministic
simulation, and two peers with different `gs.mpq` cannot stay in step.

### 1d. The state dump names a shared seed and a random-call counter

**Observed in a local binary.** The network state dump at file offsets `0x15a248`–`0x15a4f5`:

```
GS Checksum=%d,  numcomputers=%d  thiscomputer=%d  players controlled=%s
gameseed=%d      random count=%d
latency=%d  latency_total=%d  latency_samples=%d
halt_ticks=%d  average_tick_ms=%d  user_tick=%d  stop_tick=%d  skip_ticks=%d
half_speed_ticks=%d  go_countdown=%d  not_time=%d  late_messages=%d
suppress_messages=%d  total_movement=%d  any_sprite_moving=%d  stopped=%d
```

A shared `gameseed`, a `random count`, and a set of pacing counters (`halt_ticks`, `skip_ticks`,
`half_speed_ticks`, `go_countdown`, `late_messages`) is the standard instrumentation of a lockstep
loop that must stall when a peer falls behind.

**Observed in a local binary — and worth knowing before trusting the `getrandcnt` operator.**
`getrandcnt` (`0x004b5af0`) pushes the constant zero and nothing else, and the `debugbuild?`
operator has *the same entry point* — the linker folded two identical "return 0" bodies. In this
retail build the random-call counter that the desync diagnostic would use is compiled out.

### 1e. A community changelog says the same thing, from the other side

**Documented.** A comment in `DUNGEONS5.gs` in the modded corpus records that randomised map files
and tilesets were **removed** because they caused multiplayer desync. That is the mod community
independently discovering that map generation must be reproducible on every peer. (Recorded on this
repo's `research-log.md` as a Correction on 2026-09-17; cited here as corroboration, not as primary
evidence.)

### What `netlockgame` turned out to be

The brief flagged `netlockgame` as the one operator body a static walker could not complete, and as
the likely home of turn synchronisation. It is neither.

**Observed in a local binary.** Its whole body, at `0x004b6020`:

```
004b6020  mov ecx,[5D1E84h]      ; the active provider object, or null
004b6026  test ecx,ecx
004b6028  je   short 004B602F    ; no provider: return, having done nothing
004b602a  mov  eax,[ecx]         ; vtable
004b602c  jmp  dword [eax+58h]   ; nullary tail call, slot 22
004b602f  ret
```

It is a tail call through a vtable, which is precisely why a static walker stopped: the walker
correctly refuses to follow an indirect branch. It pops nothing and pushes nothing.

**Observed in a local binary.** Slot 22 on the DirectPlay class (`0x0044b440`) is one instruction:
`jmp 0x004b7dd0`, and `0x004b7dd0` is the base-class member that `thiscomputer` and `ishost` also
call — it returns the local computer's id. The operator discards the result.

**So on the transport the game actually uses, `netlockgame` locks nothing.** The Storm class has a
real implementation at `0x00471250` (host-only, calling one `STORM.dll` export with two four-byte
buffers), but see [question 2](#2-what-transport-does-it-actually-use) for why the Storm class is
not the one in play.

**Observed in the corpus.** `netlockgame` is called exactly once in the entire shipped corpus, in
`gs/Dlg/multidlg.gs`, immediately before `startnetgame`:

```
flag 1 eq{multiplayer_dialog closedialog 1 selectscenario netlockgame startnetgame}if
```

**Inferred:** it was meant to close the lobby to further joins at the moment play starts, and under
DirectPlay it silently does not.

### Correction I have to record about myself

I first reported that `0x005d1e84` is a global pointer that is **read at 110 sites and written at
none**, and concluded that the whole provider abstraction was dead code. That was wrong.

**Corrected.** `0x005d1e84` is field `+0x4b2c` of the statically allocated singleton at
`0x005cd358`. It *is* written — at `0x004b6290`, `mov [edi+4B2Ch],eax`, where `edi` is the singleton
— and no absolute store exists to find because the compiler addresses a static object's fields
absolutely on *reads* while the writer holds the base in a register. A global with many reads and no
writes is the signature of *either* an unassigned pointer *or* a static object's field; only the
arithmetic distinguishes them. `native_dispatch::static_object_field` exists so that check is one
call, and the survey tool now reports the nearest singleton below the pointer rather than every
object whose address happens to be lower.

---

## 2. What transport does it actually use?

### It is DirectPlay. The Storm network layer and both `.snp` files are not used.

The brief stated that `Battle.snp` and `Standard.snp` are DirectPlay service providers. **Refuted.**

**Observed in a local binary.** Both files are 32-bit PE DLLs exporting exactly two functions,
`SnpBind` and `SnpQuery`. That is the **Storm Network Provider** interface — Blizzard's, not
Microsoft's. Their internal module names are `STANDARD.dll` and `BATTLE.dll`, and both import from
`STORM.dll`.

**Observed in a local binary.** `Standard.snp` contains three provider descriptions:

| Provider | Description string | Source file named in the binary |
| --- | --- | --- |
| `Direct Cable Connection` | "Two or more computers connected together with serial cables and null-modems." | `SERIAL.CPP` |
| `Modem` | "Two computers, each with its own modem and phone line." | `MODEM.CPP` |
| `Local Area Network (IPX)` | "All computers must be connected to an IPX-compatible network." | `IPX.CPP` |

Its imports are `SetCommState`, `GetCommState`, `SetCommTimeouts`, `GetOverlappedResult`,
`CreateFileA` — serial ports. **It imports no sockets library at all.**

**Observed in a local binary.** `Battle.snp` is Blizzard's Battle.net client module, verbatim and
unadapted: it imports `WSOCK32.dll` (16 ordinals), contains `http://www.battle.net`,
`209.67.136.170;exodus.battle.net`, "Your connection to Battle.net has been lost.", and help text
describing a *Diablo* chat screen ("The portrait for each player displays the class and level of
their character", a ladder browser). **Inferred:** Sierra shipped Blizzard's Storm distribution as
received and never wired LOM into Battle.net.

**Observed in a local binary — the decisive check.** `lomse.exe` imports 26 ordinals from
`STORM.dll`. Every call site for all 26 lies in two modules, `0x0046f000`–`0x00471400` and
`0x004fe000`–`0x004ffb00`, plus the two allocator ordinals `#401`/`#403` which have 250 and 237
callers scattered engine-wide. It imports **three** functions from `DPLAYX.dll` — ordinals 1, 2 and
4, which that DLL's own export table names `DirectPlayCreate`, `DirectPlayEnumerateA` and
`DirectPlayLobbyCreateA` — called from `0x0044ac08`, `0x0044a809` and `0x0044a4b3`/`0x00477e04`.

And `lomse.exe` contains a complete set of C++ assertion strings naming the class `CDPlay`, with the
source-level expression of every DirectPlay call it makes — `CreatePlayer`, `EnumPlayers`,
`EnumSessions`, `GetPlayerData`, `SetPlayerData`, `Send`, `Receive`, `DestroyPlayer`, `Close`,
`Release`, plus `lpDirectPlayLobbyA->CreateAddress(...)` and `SetConnectionSettings`. There is also
`non-dplay modem not currently implemented`.

**Observed in a local binary.** There are **four** classes in the provider hierarchy, recovered
automatically by the survey tool from the constructor stores that install their vtables:

| Vtable | Constructor | Class | Object size allocated | Evidence of identity |
| --- | --- | --- | ---: | --- |
| `0x0054dbf0` | `0x004b66c0` | abstract base | — | slots 14–22 all point at one shared body (`0x0053a8a0`) |
| `0x0054d548` | `0x00449f70` | `CDPlay` | 0x4e8 = 1256 | slot 18 is `0x0044a7f0`, which calls `DirectPlayEnumerateA` |
| `0x0054d838` | `0x0046f300` | `CSigs` | 0x398 = 920 | `CSigs::ReceiveNetworkMessage() called: ERROR!`, `InitializeSIGS` |
| `0x0054d8b0` | `0x0046fb00` | Storm/SNet | 0x40c = 1036 | slot 7 `0x00470e00` sits in the module holding all the `SNET_ERROR_*` names and `SNetJoinGame failed` |

The `connect(kind)` member at `0x004b6200` on the singleton: `kind == 0` allocates 0x40c bytes and
constructs the Storm class; `kind == 1` or `2` allocates 0x398 bytes, constructs `CSigs`, and then
allocates 0x4e8 bytes and constructs `CDPlay`, replacing `CSigs` in the pointer field. The
`connecttostorm` operator passes 0; `connecttosigs` passes 2.

**Observed in a local binary.** `CSigs` is largely stubbed: five of its 28 slots, including
**`selectprovider`** (slot 8) and **`joinnetworkgame`** (slot 3), point at `0x0044b450`, which is
`xor eax,eax / ret 4` — return failure. **Inferred:** the Sierra Internet Gaming Service path was
non-functional by the time this build shipped, which is unsurprising given that WON shut down in
2007.

**Observed in the corpus.** The shipped service-provider dialog, `gs/Dlg/NetSpDlg.gs`, calls
`connecttostorm` and, if it returns false, returns to the main menu. A second dialog,
`gs/Dlg/netdlg.gs`, offers a choice between `connecttostorm` and an operator called
`connecttodplay` — and **`connecttodplay` does not exist in the engine's operator table**
(Observed: it is absent from all 1,908 recovered operator names). So `netdlg.gs` is vestigial
developer script referencing a removed operator.

**Unknown.** I did not establish why the reachable entry point is called `connecttostorm` while the
object it produces is the Storm class and the object that `connecttosigs` produces is `CDPlay`. The
naming and the construction do not line up, and I did not trace which one the shipped menu chain
actually reaches at run time. This is the largest hole in question 2 and it is one click to settle
in-game — see [what needs a second machine](#what-would-need-testing-with-a-second-machine).

### Which providers, in what order, and the mislabelling hazard

**Observed in a local binary.** `enumproviders` (`0x004b8200`) is vtable slot 18; on `CDPlay` that
is `0x0044a7f0`, which zeroes a count at object offset `+0x2c8` and calls `DirectPlayEnumerateA`
with the callback `0x0044a7c0`. `providername` (`0x004b8260`) reaches the base-class member
`0x004b7df0`, which bounds-checks against `[+0x2c8]` and indexes 50-byte records at object offset
`+0x134`; `(0x2c8 - 0x134) / 50 = 8`, so **the provider list holds at most 8 entries**.
`selectprovider` (slot 8, `0x0044abc0`) indexes a GUID-pointer array at object offset `+0x394` and
calls `DirectPlayCreate(provider_guid[n], ...)`.

**Observed in a local binary.** The DirectPlay application GUID is
`{845228F0-B9E2-11D1-A463-00A024D134DF}`, at `0x00556898`. `EnumSessions` is called at `0x0044b0cd`
with a `DPSESSIONDESC2` of `dwSize = 0x50`, this GUID as `guidApplication`, flags `1`
(`DPENUMSESSIONS_AVAILABLE`), and — the constant is `push 32h` at `0x0044b111` — a **timeout of 50
milliseconds**.

**Observed in the corpus — and this is the mislabelling hazard.** `gs/Dlg/NetSpDlg.gs` does not use
the engine's provider names unless there are more than four providers:

```
/provider_table4["DIRECT CABLE""MODEM""LOCAL AREA NETWORK""WON.NET"]def
/getprovidername{... numproviders 4 gt{providername}{lom_spnames exch get}ifelse ...}
```

With exactly four providers the labels are assigned **by position** from that hardcoded table, the
last index is hidden from the list, and provider index 1 — whatever it actually is — is routed to
the modem dial-up dialog (`netselectmodemgame`, with a 25-character phone-number field).

**Observed on this machine.** The Wine prefix registers exactly four DirectPlay service providers,
under `Software\Microsoft\DirectPlay\Service Providers` in `system.reg`, in this stored order:

| Registry order | Name | `Guid` | `Path` | Present in prefix? |
| ---: | --- | --- | --- | --- |
| 1 | Internet TCP/IP Connection For DirectPlay | `{36E95EE0-8577-11cf-960C-0080C7534E82}` | `dpwsockx.dll` | **yes** |
| 2 | IPX Connection For DirectPlay | `{685BC400-9D2C-11cf-A9CD-00AA006886E3}` | `dpwsockx.dll` | **yes** |
| 3 | Modem Connection For DirectPlay | `{44EAA760-CB68-11cf-9C4E-00A0C905425E}` | `dpmodemx.dll` | **no** |
| 4 | Serial Connection For DirectPlay | `{0F1D6860-88D9-11cf-9C4E-00A0C905425E}` | `dpmodemx.dll` | **no** |

**Inferred.** The hardcoded label order only makes sense if `DirectPlayEnumerate` yields
`Serial, Modem, IPX, TCP/IP` — then `DIRECT CABLE`→Serial, `MODEM`→Modem, `LOCAL AREA
NETWORK`→IPX, and `WON.NET`→TCP/IP, which is also why index 3 is excluded from the plain button
list and offered only through the separate `wonisenabled?` button that calls `connecttosigs`. On
this installation the registry stores them in the opposite order.

**Unknown.** Whether Wine's `DirectPlayEnumerateA` returns registry order. I did not read Wine's
`dplayx` enumeration. If it does, every button in that dialog selects the wrong transport, which is
a one-click test.

### How a session is found, and whether you can type an address

**Observed in the corpus — and this is the load-bearing answer for the homelab question.** The
shipped game-selection dialog is `gs/Dlg/NetGmDlg.gs`. It contains:

- one 20-character edit box for a **game name**;
- a list of up to 100 discovered sessions (`bigasstempstringarray[100]`, scrolled 10 at a time),
  filled entirely from `enumnetworkgames` and `networkgamename`;
- `Create` (gated on `checkforcd`) and `Join`, where `Join` first runs `checkgameexists`, which
  linearly matches the typed name against the enumerated names and does nothing at all if there is
  no match;
- `njoin` = `"RemoteComputer" setcomputername enumnetworkgames pop joinnetworkgame`, i.e. join by
  *index into the enumeration*.

**There is no IP-address field anywhere in the shipped multiplayer UI.** The only address a user can
type is a modem phone number, in `netselectmodemgame`.

**Observed in a local binary.** The exe *can* build a DirectPlay address from a host string — the
`CreateAddress(SP, DataType, HostAddress, ...)` / `SetConnectionSettings` pair at
`0x0044a69f`/`0x0044a6f7` — and `connect()` formats an IPv4 address with `%d.%d.%d.%d`
(`0x004b639c`) from a 32-bit value fetched out of another object and hands it to a `CDPlay` member
at `0x0044a030`. **Observed:** that path is reached only from the `connecttosigs` branch, where the
address comes from a SIGS-supplied game list, not from the user. **Inferred:** directed-IP join is
implemented in the engine and unreachable from the shipped UI.

**Documented.** Microsoft's own `dpwsockx.dll` presents its own "enter host IP" dialog when
DirectPlay enumerates with no address, which is how most DirectX-6-era games let a user type an
address without the game having a field for it. **Observed on this machine:** Wine's `dpwsockx.dll`
(version 5.3.1.904) has *no dialog resource and no user-visible strings at all* — its only wide
strings are the version block. So on this installation that escape hatch does not exist.

### Ports

**Observed on this machine.** Wine's `dpwsockx.dll` imports `socket`, `bind`, `listen`, `accept`,
`connect`, `sendto`, `WSASendTo`, `WSASend`, `WSARecv`, `setsockopt`, `htons`, `htonl` from
`ws2_32.dll`. So the provider speaks both TCP and UDP; `dplayx.dll` itself imports no sockets
library, so all transport lives in the provider.

**Observed on this machine.** The 32-bit constants `47624`, `2300` and `2400` appear as immediates
in `dpwsockx.dll`'s code section at file offsets `0x1c92`, `0x335f` and `0x3405`.

**Documented.** Microsoft's DirectPlay TCP/IP provider uses TCP 47624 for session enumeration and
TCP+UDP 2300–2400 for session traffic, and broadcasts to the local subnet when enumerating without
an address. **Inferred**, from the three constants above, that Wine's provider uses the same
numbers. I did not verify the broadcast behaviour in Wine's code, and I did not put a packet on a
wire.

---

## 3. What "touchy" actually consists of

Every item here is a concrete mechanism with an address or a corpus citation. Together they are the
answer to "why is it like this".

### 3a. There is a version and content handshake, and mixed installs fail it

**Observed in a local binary.** The six divergence classes in 1a include `'EXE' version`,
`'IMP' files` and `'GS' files`. `lomse.exe` also contains the message types `COMPUTER_INFO` and
`SAVED_GUIDS`, the operator `matchguids`, the strings `Lords of Magic v1.0` and `1.0.7`, and
`Host ignores COMPUTER_INFO`.

**Inferred, and directly relevant to a modding community:** a peer running the unofficial 3.02
patch, a peer running GS5R3 and a peer running vanilla have different `gs.mpq`, so they will be
reported as having different `'GS' files`. **Everyone in a session must run byte-identical `gs.mpq`,
`imp.mpq` and `lomse.exe`.** This is the single most actionable finding in the document.

**Unknown.** Whether the mismatch is refused at join time or only reported as a divergence once play
begins. The strings read like a report, not a refusal, but I did not trace the control flow after
the report.

### 3b. Network messages are capped at 257 bytes, on a fixed buffer

**Observed in a local binary.** The receive path is vtable slot 21; on `CDPlay` that is
`0x0044b130`. It writes `mov dword [eax],105h` at `0x0044b160`, where `eax` is `message + 0x101`,
and passes `message` as `lpData` and `message + 0x101` as `lpdwDataSize` to `IDirectPlay3::Receive`.
The send path (slot 20, `0x0044b210`) passes `[message + 0x101]` as the length at `0x0044b334`.

So the size field lives at `+0x101` and the declared capacity is `0x105` = 261. **Inferred:** the
payload region is 257 bytes and the four extra bytes of declared capacity are the size field itself,
so a 261-byte inbound message would overwrite its own length. `lomse.exe` also carries a
`Buffer Too Small` string at file offset `0x154ea0`.

**Inferred:** the `REQUEST_SCENARIO` / `XFER` / `XFER_PROGRESS` message family exists because a
scenario file cannot fit in 257 bytes.

### 3c. The send path sleeps on the game thread, and retries

**Observed in a local binary**, all inside `0x0044b210`:

- `Send` is called with flags `9` at `0x0044b34b` — `DPSEND_GUARANTEED | DPSEND_OPENSTREAM`
  (**Documented** for the constant values; the assertion string at file offset `0x155424` names them
  in source form), from `ThisComputerDPID` to one specific `lpcomputer->dpId`. **Observed: it is a
  full peer-to-peer mesh of directed reliable sends. There is no server.**
- Before each send, at `0x0044b2ee`–`0x0044b310`: `GetTickCount`, subtract the last send tick, and
  if a configurable minimum gap `[this+0x388]` has not elapsed, **`Sleep` the difference**. The send
  path blocks the main thread to rate-limit itself.
- After each send, `[this+0xcc]` selects either `Sleep([this+0x384])` or
  `WaitForSingleObject(event, [this+0x384])`, then pumps messages via `0x004b7e20`.
- The whole thing loops `[this+0x380]` times — a retry count.
- The base constructor at `0x004b66c0` sets `[this+0x384] = 0x64`, i.e. **a 100 ms post-send wait by
  default**.

**Unknown.** Where `[this+0x380]` (retry count) and `[this+0x388]` (minimum inter-send gap) are
initialised. They are not set in the base constructor and I did not find their writers.

### 3d. The per-destination message sequence number is one byte

**Observed in a local binary.** At `0x0044b2ba`–`0x0044b2d3`: a per-destination counter array at
`0x005d1f0c`, indexed by computer id, incremented, and **its low byte only** written into the
message at offset `+5`. The receive side has the string
`Error: expected message# %d from %d, got %d` at file offset `0x15b6d4`, pushed at `0x004b8da7`.

**Inferred:** the wire sequence number wraps every 256 messages while the in-memory counter does
not. Whether the comparison handles the wrap is **Unknown** — I did not read `0x004b8da7`'s
surrounding comparison.

### 3e. Sends are silently dropped for a flagged peer

**Observed in a local binary.** At `0x0044b28c`: if the byte at `[computer_record + 0x21]` is
non-zero, `SendNetworkMessage` returns success (`eax = 1`) **without sending anything**. There are
two other early returns that at least log — `No ThisComputerDPID` when the local DPID is `0xFFFF`
and `No dpId` when the target's is — but this one is silent.

### 3f. `netlockgame` and the whole operator family are silent no-ops without a session

**Observed via the survey tool.** All eleven operators that dispatch virtually on the provider
pointer — `createnetworkgame`, `joinnetworkgame`, `modemcreate`, `modemdial`,
`enumnetworkcomputers`, `enumnetworkgames`, `selectprovider`, `enumproviders`, `netlockgame`,
`getproviderbackground`, `getproviderbuttontexture` — null-check the pointer and return quietly when
it is null. `enumproviders` returns **1** in that case, and `providername` then yields the string
`error getting providername` (file offset `0x15b638`, pushed at `0x004b82e2`).

**Inferred:** after a dropped session, multiplayer script keeps running and every network operator
quietly does nothing, rather than raising. That matches the brief's suspicion exactly.

### 3g. There is a real-time turn clock and a real-time combat mode

**Observed in the corpus.** `gs/Dlg/multidlg.gs` exposes `TURN_LIMIT` with the values
`30 Seconds`, `1 Minute`, `3 Minutes`, `5 Minutes`, `Unlimited`, and `lomse.exe` carries
`Time left = %d Seconds` (file offset `0x159430`) and `RealTimeNetworkCombat - p1 or p2 is NULL?`
(`0x159448`). `COMBAT_MODE` has three values: `Always Autocalc Combat`,
`Only Observe Player Combat`, `Observe All Combat`.

**Inferred:** simultaneous real-time turns on a shared clock, plus real-time tactical combat
replicated as an event stream (`ADD_MISSILE`, `ARMY_ANIM`, `ARMY_KILLUNIT`), is the hardest possible
case for lockstep determinism. Setting `COMBAT_MODE` to `Always Autocalc Combat` removes the
real-time combat simulation from the shared state entirely, and is the one option in the dialog that
plausibly reduces desync exposure. **Unknown** whether it actually does; that is a play test.

### 3h. The full list of multiplayer options

**Observed in the corpus.** Ten names are passed to `get`/`set`/`inc`/`decmultiplayeroption` across
the whole corpus, and there are no others:

| Option | What it controls (from the dialog) | Host-only? |
| --- | --- | --- |
| `SCENARIO_MODE` | play a `.scn` from `map/` or resume a save from `multisav/` | yes |
| `SHROUD` | line of sight on the scrolling map, location, region and world screens | yes, and only in scenario mode |
| `STRONGHOLD` | stronghold level toggle | yes, scenario mode only |
| `AI_PLAYERS` | number of AI players | yes, scenario mode only |
| `TURN_LIMIT` | 30s / 1m / 3m / 5m / unlimited | yes |
| `STARTING_GOLD` | starting gold | yes, scenario mode only |
| `STARTING_CRYSTALS` | starting crystals | yes, scenario mode only |
| `STARTING_ALE` | starting ale | yes, scenario mode only |
| `SPELLS_KNOWN` | spells granted at start, from a table | yes, scenario mode only |
| `COMBAT_MODE` | autocalc / observe player combat / observe all | yes |

`getmultiplayerflag` / `setmultiplayerflag` is a single boolean — "this is a network game" — read in
thirteen script files.

### 3i. Player and computer caps, and the map restriction

**Observed in the corpus.** `gs/Dlg/multidlg.gs` builds eight player slots (`0 1 7 for`, eight
portraits, eight name boxes) and `resetnetworking 8 newgame`. **Eight player slots.**

**Observed in a local binary.** The divergence reporter at `0x004b4fc0` indexes computer records at
base `0x005af194` with stride 7556 bytes and bounds-checks `index - 1` against `0x10`, so
**computer ids run 1..16**. `thiscomputer` (`0x004b7dd0`) returns 1 when `numcomputers < 2`.
`numcomputers` (`0x004b8b10`) reads `[provider + 0x100]` with a floor of 1.

**Observed in the corpus.** The host's Play button refuses to start unless
`enumnetworkcomputers >= 2`, every joined player has a non-empty name, and
`enumnetworkleaders >= enumnetworkcomputers`.

**Observed in the corpus.** The multiplayer scenario list only offers a `.scn` whose placeholder
scan finds **all eight faiths** (`checkfor8faiths 8 eq`). A custom map with fewer than eight faiths
cannot be selected for multiplayer at all.

### 3j. Session discovery has a 50 ms timeout

Repeated here because it belongs on this list: `EnumSessions` is called with `dwTimeout = 50`
(**Observed**, `push 32h` at `0x0044b111`). **Documented:** that parameter is how long DirectPlay
waits for session replies. **Inferred:** 50 ms is a LAN-only budget; a peer whose round trip exceeds
it will simply not appear in the list, and since `Join` requires an exact name match against that
list, an unlisted host is unjoinable.

---

## 4. What a homelab could — and could not — do

The user's expectation, restated from the brief: there is no dedicated server binary, so the
realistic homelab role is an always-on Windows VM acting as the host peer plus a network that makes
all players look like one LAN.

**That expectation is confirmed as far as it goes, and it does not solve the stated problem.**

### Confirmed

- **There is no dedicated server binary. Observed.** The game directory contains `lomse.exe`,
  `DPStub.exe` (35,328 bytes, which imports only `DirectPlayLobbyCreateA` plus
  `RegCreateKeyA`/`RegSetValueExA`/`RegDeleteKeyA`/`ShellExecuteExA` — it is a DirectPlay lobby
  registration stub, not a server), `storm.dll`, two `.snp` files and the launcher. No other
  executable. The transport is a peer-to-peer mesh of directed `Send` calls
  ([3c](#3c-the-send-path-sleeps-on-the-game-thread-and-retries)).
- **Hosting is one peer running the game. Observed.** `ishost` (`0x004b5e80`) compares the provider
  object's field `+0x390` against `thiscomputer`; the host is simply the computer that called
  `createnetworkgame`.
- **You need one broadcast domain, not merely routed reachability. Observed + Documented.** The UI
  offers no way to type an address ([question 2](#how-a-session-is-found-and-whether-you-can-type-an-address)),
  so discovery is whatever the DirectPlay provider does on its own, which is **Documented** as
  subnet broadcast. A layer-2 VPN (or a bridged overlay putting every player in one subnet) is
  therefore required, and routed-only connectivity is not enough.
- **The ports to pass, if you run the TCP/IP provider: TCP 47624, and TCP+UDP 2300–2400**
  (**Documented** for the meaning; **Observed** that those three constants are present in the
  installed `dpwsockx.dll`).

### Why it still does not fix "touchy"

**The dominant failure mode is desync, and desync is not a network property.** From
[question 1](#1-lockstep-deterministic-or-host-authoritative), the engine independently simulates
the world on every peer and compares six checksums. A perfect network delivers the same orders in
the same order to every peer, and two peers with a different `gs.mpq`, a different `imp.mpq`, a
different `lomse.exe`, or a different animation frame count still diverge. Lower latency does not
help. Zero packet loss does not help. Being on the same VLAN does not help.

What *does* help is content identity and reducing the amount of real-time simulation in the shared
state — which are configuration decisions, not infrastructure:

1. **Byte-identical `gs.mpq`, `imp.mpq`, `pic.mpq` and `lomse.exe` on every peer.** This is the
   highest-value action available and it needs no homelab at all. Distribute the files, do not tell
   people which mod to install. (**Inferred** from [3a](#3a-there-is-a-version-and-content-handshake-and-mixed-installs-fail-it).)
2. **`COMBAT_MODE = Always Autocalc Combat`**, to keep real-time tactical combat out of the
   replicated stream. (**Inferred** from [3g](#3g-there-is-a-real-time-turn-clock-and-a-real-time-combat-mode).)
3. **Identical Wine/Windows and renderer configuration**, because `'PlayAnimation Count'` is a
   divergence class ([1a](#1a-the-engine-compares-six-checksums-between-peers-and-calls-divergence-disagreement)).
   A peer with a different frame-pacing or `cnc-ddraw` setting is an unnecessary risk. **Unknown**
   how strongly it matters; it is a play test.
4. **A `.scn` with all eight faiths**, or the map will not even be selectable
   ([3i](#3i-player-and-computer-caps-and-the-map-restriction)).

### Where a homelab genuinely earns its place

Not as a fix, as a reduction in setup friction:

- **An always-on Windows VM as the host peer.** Real value: the host does not have to be someone's
  desktop, the game can sit in the lobby, and the host's copy of `gs.mpq`/`imp.mpq`/`lomse.exe`
  becomes the single canonical build everyone else syncs from. That last part attacks the *actual*
  failure mode.
- **A layer-2 overlay so every player is in one subnet.** This is required, not optional, given
  there is no address field. A routed WireGuard tunnel is not sufficient; a bridged/TAP overlay or
  an L2 VPN is.
- **A file-serving role.** Serve the canonical archives from the same box, so "run the same build"
  is enforced by the distribution mechanism.
- **`RESYNC_REQUEST` exists** ([1c](#1c-what-crosses-the-wire-is-orders-not-state)), so a
  well-connected host may recover a desynced peer more often than a flaky one would. **Inferred and
  weak** — I did not read the resync implementation and cannot say whether it works.

### One honest alternative worth weighing

**Inferred.** Because the whole design is lockstep peers with a 257-byte message cap and a 50 ms
enumeration timeout, the least-effort route to a smooth session is probably not a homelab at all:
put every player on the *same* machine's LAN, or run all peers as VMs on the homelab host with the
players connecting by remote desktop. That eliminates the network as a variable entirely and leaves
only determinism, which is the part that actually breaks. It is worth weighing against building an
L2 overlay for a game whose discovery budget is 50 ms.

---

## What would need testing with a second machine

Everything above is static. These are the experiments only the user can run, ordered by how much
they would change the picture. Each names what result would mean what.

1. **Does the service-provider dialog label the providers correctly?**
   Open Multiplayer and read the buttons. If you see three buttons labelled `DIRECT CABLE`, `MODEM`,
   `LOCAL AREA NETWORK`, note which one, when clicked, produces the modem *phone number* dialog. It
   should be `MODEM`. If it is a different button — or if `LOCAL AREA NETWORK` gives you a dial-up
   box — then `DirectPlayEnumerate` returns registry order and every label is wrong, which settles
   the **Unknown** in [question 2](#which-providers-in-what-order-and-the-mislabelling-hazard). If
   the list is empty or shows `error getting providername`, `connecttostorm` failed and the provider
   object is null ([3f](#3f-netlockgame-and-the-whole-operator-family-are-silent-no-ops-without-a-session)).
   *This is one click and it resolves the biggest gap in the document.*

2. **Which class actually serves the session?**
   Reaching "Multiplayer" goes through `connecttostorm`. If a session can be created and joined at
   all, `CDPlay` or the Storm class is live; the distinguishing observable is whether a
   packet capture on the host shows traffic on TCP 47624 / 2300–2400 (DirectPlay) or something else
   (Storm). This settles the naming/construction mismatch flagged as **Unknown** in question 2.

3. **Two peers with deliberately different `gs.mpq`.**
   Host with vanilla, join with the unofficial 3.02 patch. Predicted: a message naming
   `different 'GS' files`. What matters is *when* — at join, or only after play begins. That settles
   the **Unknown** in [3a](#3a-there-is-a-version-and-content-handshake-and-mixed-installs-fail-it),
   and it is the difference between "mixed installs are refused" and "mixed installs quietly
   corrupt the game".

4. **Two peers with identical everything, playing a full game with `COMBAT_MODE = Always Autocalc`,
   then the same game with `Observe All Combat`.**
   Predicted: divergence is markedly more likely in the second. This is the only way to test
   [3g](#3g-there-is-a-real-time-turn-clock-and-a-real-time-combat-mode), and it is the advice most
   worth verifying because it is the advice a player can act on.

5. **Does `netlockgame` leaving the lobby unlocked matter?**
   After the host presses Play, try to have a third machine join. If it can,
   [3f](#3f-netlockgame-and-the-whole-operator-family-are-silent-no-ops-without-a-session) is
   confirmed as a live defect rather than a dormant one.

6. **Enumeration across a routed hop versus a bridged overlay.**
   Two peers on different subnets with routing between them: predicted, no sessions listed, because
   discovery is broadcast and there is no address field. Same two peers on a bridged L2 overlay:
   predicted, the session appears. This is the experiment that confirms or refutes the one genuine
   homelab requirement in [question 4](#where-a-homelab-genuinely-earns-its-place).

7. **A session with more than eight players, and with more than eight computers.**
   The script builds eight player slots; the divergence reporter indexes sixteen computer records.
   What happens between 9 and 16 is **Unknown** and only observable by trying it.

8. **Capture the network log.**
   `gs/network.gs` registers a full state dump through `netlog` and `setstatlogproc`. If that log
   can be made to land in a file, two peers' logs after a divergence identify the exact army and
   unit that diverged. I did not find where `netlog` writes; establishing that would turn every
   future desync report into a diffable artefact, and is probably the highest-leverage thing anyone
   could do for this game's multiplayer.
