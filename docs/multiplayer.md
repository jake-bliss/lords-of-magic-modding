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

8. **Capture the network log.** *Superseded — see
   [the desync post-mortem is disabled](#5-the-desync-post-mortem-is-wired-up-and-disabled). The
   answer is that it cannot be captured, and no second machine is needed to know that.*

---

# Second pass: the six checksums, the pre-flight checker, and the disabled post-mortem

Everything in this half was recovered after the first, again with no game run and no packet sent.
Two results here **sharpen** the first half and one **replaces** a hope stated in it. Nothing in
the first half is refuted.

Reproduce with:

```
cd spikes/asset-viewer
cargo run --release --example preflight -- "<install A>/English" "<install B>/English" ...
cargo run --release --example multiplayer_survey -- <lomse.exe>
```

## 5. The six divergence values, individually

**Observed in a local binary.** The comparator dispatches on a value index through a six-entry
jump table at `0x004b54a8`, guarded by `cmp eax,5 / ja` at `0x004b4f9b`. Matching each jump target
to the string its branch pushes gives the index-to-class map:

| Index | Jump target | Divergence class |
| ---: | ---: | --- |
| 0 | `0x004b51ef` | `'EXE' version` |
| 1 | `0x004b5027` | `'GS' files` |
| 2 | `0x004b5110` | `'Random-Seed'` |
| 3 | `0x004b4fb6` | `'Player-Stats'` |
| 4 | `0x004b509f` | `'IMP' files` |
| 5 | `0x004b5181` | `'PlayAnimation Count'` |

**Observed.** Three of the six message texts say `has a different …` and three say
`now has a different …`. The three without `now` are `'EXE' version`, `'IMP' files` and
`'GS' files` — the install-derived ones. That reading is corroborated by what the code computes.

### The one message builder, and everything it puts in the message

**Observed in a local binary.** There is exactly one function that builds a `CHECKSUM` message
(`0x004b4c20`) and it has exactly one caller (`0x0048bdf5`). Its complete set of payload stores:

| Payload offset | Written with | At |
| ---: | --- | ---: |
| `+0x31` | `thiscomputer` | `0x004b4d79` |
| `+0x35` | first argument | `0x004b4d75` |
| `+0x39` | *never written* | — |
| `+0x3d` | `[0x00583560]`, the tick — then **overwritten** by the second argument | `0x004b4d88`, `0x004b4da6` |
| `+0x41` | `[0x00584424]`, the executable byte sum | `0x004b4d6b` |
| `+0x45` | *never written* | — |
| `+0x49` | `[0x00584604]`, the GameScript content checksum | `0x004b4d96` |
| `+0x4d` | `[0x00573428]`, the game seed | `0x004b4d9e` |
| `+0x51` | the result of the script's `setchecksumproc` procedure | `0x004b4d92` |
| `+0x55` | zero | `0x004b4d9a` |
| `+0x59` | zero | `0x004b4da2` |

Ten dwords, which matches the receiver copying 40 bytes (`mov ecx,0Ah / rep movsd` at
`0x004b4e88`) into a 40-byte-strided per-computer record.

**Observed.** Both stores at `0x004b4d88` and `0x004b4da6` encode `[esp+0x3d]` — raw bytes
`89 4c 24 3d` and `89 44 24 3d`. The tick is computed, stored, and thrown away; the CHECKSUM
message does not carry the tick it was taken at. (The divergence *report* gets the tick from the
global directly at `0x004b526e`, so the printed `at Tick #%d` is the tick of the *comparison*, not
of the checksum.)

**Inferred**, and this is the load-bearing inference of this section: only **four** meaningful
quantities are compared — the executable byte sum, the GameScript content checksum, the game seed,
and the script-supplied state checksum. Two slots are hardcoded zero and two are never written at
all, so **two of the six named divergence classes can never fire, and a third may compare
uninitialised stack.** Which class lands on which slot I could not establish; the payload offsets
and the value indices do not line up in any ordering I could justify, and I am not going to pick
one to make the table tidy. **Unknown.**

### `'EXE' version` — reproducible exactly

**Observed in a local binary**, `0x004b4a95`–`0x004b4b2a`. The engine calls
`GetModuleFileNameA(NULL, …)`, opens that path `"rb"`, reads the whole file, and accumulates into
the global `0x00584424`:

```text
004b4b0c  mov [584424h],ebx        ; the accumulator starts at zero
004b4b14  mov ecx,[584424h]
004b4b1a  xor edx,edx              ; zero-extend
004b4b1c  mov dl,[eax+edi]
004b4b1f  add ecx,edx
004b4b21  inc eax
004b4b22  cmp eax,esi
004b4b24  mov [584424h],ecx
004b4b2a  jb  004B4B14h
```

A 32-bit wrapping sum of the **zero-extended** bytes of the running executable. Reproduced by
`install_checksum::exe_checksum`.

### `'GS' files` — the algorithm is reproducible, the value is not

**Observed in a local binary**, `0x004d496c`–`0x004d4990`, inside the GameScript loader path:

```text
004d496c  mov eax,[584600h]        ; the gschecksumon / gschecksumoff flag
004d4971  test eax,eax
004d4973  je  004D4992h            ; flag clear: accumulate nothing
004d497b  movsx edx,byte [eax+edi] ; SIGN-extend
004d497f  mov ebp,[584604h]
004d4985  add ebp,edx
004d4987  inc eax
004d4988  cmp eax,ecx
004d498a  mov [584604h],ebp
004d4990  jl  004D497Bh
```

So `'GS' files` is **not** a digest of `gs.mpq`. It is a running sum of the bytes of every script
source the loader is handed, and `0x00584604` is the same global the state dump prints as
`GS Checksum=%d`. **This sharpens the first half rather than contradicting it:** peers with
different scripts still diverge, and the engine still notices — it notices by executing them, not
by hashing the archive.

Note `movsx`, against the executable sum's `xor edx,edx`. **The two byte sums are not the same byte
sum.** A byte of `0x80..=0xFF` contributes `+128..=+255` to one and `-128..=-1` to the other. Any
test vector made of ASCII cannot tell them apart, which is exactly how a guessed hash would have
survived a weak test.

**Observed.** The engine brackets the accumulation: `0x004ff59c` zeroes `0x00584604`, sets
`0x00584600`, calls the loader at `0x004d48a0`, and clears the flag again at `0x004ff5b8`. So the
value covers exactly the members one top-level script run reaches. **Which members that is, is
Unknown** — it is a property of the script graph, not the archive. And because `gschecksumon` is a
script-callable operator, a mod can widen the bracket and change the value without changing any
script byte.

### `'IMP' files` — no algorithm found

**Observed.** `lomse.exe` contains no `.mpq` filename string at all, and the only two whole-file
byte-sum routines in the image are the executable sum above and a generic one at `0x004b1e60`
(identical idiom, unsigned, filename obtained by running the script's `setscenarionameproc`
procedure) which serves the scenario-file transfer and writes its results to `0x00583ad4`/
`0x00583ad8`. Neither touches `imp.mpq`.

**Inferred:** `'IMP' files` is one of the four dead payload slots and is not computed in this build.
I could not prove which slot, so this is inference, not observation. It also means **the
coordinating expectation that `'GS' files` and `'IMP' files` are archive digests is only half
right**, and the pre-flight checker is built accordingly rather than around a guessed hash.

### The comparison is bounded, and the bounds are small

**Observed in a local binary.**

- The checksum queue is **100 entries of 192 bytes**: the reset at `0x004b4a50` runs
  `mov esi,64h` (100) over a body that advances `edx` by `0xc0` each iteration. That is the queue
  behind `Error: Checksum queue is full for ID #%d...` (`0x004b4dff`).
- The comparison covers **at most four computers**: `cmp ebx,4 / jge` at `0x004b4f43`, walking a
  `0x28` stride. Computer records themselves are indexed `1..16` (`cmp ecx,10h / jge` at
  `0x004b4fbd`, base `0x005af194`, stride 7556 bytes). **Inferred: in a session with more than four
  computers, the fifth and beyond are not checked against anyone.**

### Which messages trigger a checksum

**Observed in a local binary.** A byte table at `0x0048c000` indexed by `gm_type - 0x0d`, feeding a
jump table at `0x0048bfd8`, routes 16 of the 80 dispatched types to the CHECKSUM-emitting branch at
`0x0048bdeb`. Shift-corrected (see below), they are: `MOVE_ARMY`, `END_TURN`, `BUY_UNIT`,
`TRANSFORM_UNIT`, `SET_PLAYER_DATA`, `START_COMBAT`, `END_COMBAT`, `GO`, `ORDERS`, `BATCH_ORDERS`,
`UNPAUSE_ARMY`, `UNPAUSEALL_ARMY`, `REENTER`, `NETMERGE`, `ENTER_BUILDING`, `SCRIPTCALLBACK`.

Every one of those is a simulation mutation, which is a third independent confirmation of the name
shift: corrected, the list is coherent; uncorrected, it is a nonsense mix.

## 6. The message-name table is off by one, and its last slot is a wild pointer

**Observed in a local binary.** The lookup at `0x0048a4e0`:

```text
0048a4e0  mov eax,[esp+4]
0048a4e4  test eax,eax
0048a4e6  jl  0048A4F5h            ; negative -> "Unknown: gm_type %d"
0048a4e8  cmp eax,62h              ; 98
0048a4eb  jge 0048A4F5h
0048a4ed  mov eax,[eax*4+55B898h]
0048a4f4  ret
```

The bound and the table base are both read out of that body by `multiplayer_survey`, which then
dumps the table. **Observed: the bound permits `gm_type` 0..97, and only 97 slots resolve to a
string.** Slot 97 holds `0x52454658`, the ASCII bytes `XFER`: the pointer table has run into the
string data it points at. The caller formats the result with `%s`.

**Observed.** Slot 20 is `THIEF_STOLEN_RESOURCEPRISONER_ESCAPE`, 36 characters — the longest entry
in the table by eight characters over the next longest, `UPDATE_SPELLS_USED_IN_COMBAT`.

**Inferred**, strongly: that is two names in one C string literal, i.e. a missing comma, so the
table has 97 pointers where the enum has 98 values, and `table[t]` yields the name of message
`t + 1` for every `t ≥ 21`.

**Observed — an independent cross-check that shares no mechanism with the string evidence.** The
CHECKSUM builder sends `push 5Fh` at `0x004b4c4f`, i.e. `gm_type` **95**. Table slot 94 is
`CHECKSUM`; slot 95 is `AUTOPLAY`. A live send site puts the name exactly one slot low, which
confirms the shift from code rather than from the data layout. The same arithmetic makes the
intended type of `XFER_PROGRESS` 97 — the one index the bound permits and the table cannot serve.

**Inferred:** the divergence report, whose whole job is to say which message the peers disagreed
after, prints the wrong message name for anything above type 20, and prints from a wild pointer for
`XFER_PROGRESS`. The wrong names are all plausible, which is the worst kind of wrong.

A first version of the survey tool tried to detect the merge automatically by "entry X ends with
entry Y's whole name". That rule fired on `BATCH_ORDERS`/`ORDERS`, `SCRIPTCALLBACK`/`ACK` and
`REQUEST_START_GAME`/`START_GAME` — all legitimate — and missed slot 20, because the swallowed name
is only a *suffix* of the merged literal and no slot points at it. The tool now reports the length
distribution and leaves the argument to this document.

## 7. The desync post-mortem is wired up and disabled

This replaces the first half's hope that the network log could be captured. **It cannot be, and the
reason is in the binary.**

**Observed in a local binary.**

```text
004b4940  mov eax,[5843E4h]        ; the gate
004b4945  test eax,eax
004b4947  je  004B496Ah            ; zero: return, having done nothing
004b4949  mov eax,[5843E8h]        ; the setstatlogproc procedure
004b494e  cmp al,6                 ; tag 6 = procedure
004b4950  jne 004B496Ah
004b4957  mov ecx,[5843ECh]
004b495d  push ecx
004b495e  mov ecx,[5A7B78h]        ; the interpreter
004b4965  call 004D05E0h           ; execute it
```

- **Observed.** `0x004b4940` has exactly one caller: `0x004b52d7`, immediately after the
  `Divergence` report is formatted. The engine is built to dump full state at the moment a desync
  is detected.
- **Observed.** `0x005843e8`/`0x005843ec` hold the procedure `setstatlogproc` registers
  (`0x004b5781`, `0x004b57b4`), and `gs/network.gs` registers one that walks every player, army and
  unit through `netlog`.
- **Observed.** `0x005843e4`, the gate, is written at exactly one instruction in the whole image —
  `0x004b585e`, `mov [5843E4h],edi`, inside the `netlog` operator, with `edi` zeroed at
  `0x004b57d9`. It lies past the end of `.data`'s raw data, so it is zero at load.

**So the gate is zero at load and the only write to it writes zero. The post-mortem never runs.**

This time that reading is safe, and it is worth saying why, because the first half of this document
records me getting an identical-looking measurement wrong. `0x005d1e84` had many reads and no
absolute writes because it is a *field of a static object* whose writer holds the base in a
register. `0x005843e4` is different: it *is* written absolutely, and its neighbours
`0x005843e0`/`0x005843e8`/`0x005843ec`/`0x005843f0` are each written absolutely by a different
operator, which is the signature of separate globals rather than one object. The
absolute-write-exists check is what makes the conclusion sound here and unsound there.

**Observed.** `netlog` itself (`0x004b57d0`) pops one string and, if its first byte is `*`
(`cmp byte [eax],2Ah` at `0x004b5859`), stores zero to the gate. It contains no output call of any
kind — its only `call` is the interpreter's error raiser. **`netlog` writes nothing.**

**Inferred:** this is why no one in this community has ever diffed a desync. The instrument exists,
fires in the right place, and is switched off in the shipped binary with no switch to turn it on.

### What is *not* the same thing

- **Observed.** `GS.LOG` (`0x0055e44c`) and `GSDEBUG.DAT` (`0x0055e440`) are filename fields of the
  GameScript **VM** object, copied in at `0x004d2225` and `0x004d221d` into fields `+0x660` and
  `+0x55c`. They are the script engine's own logs, not the network stat log. **Unknown:** what
  writes them and under what condition.
- **Observed.** The `GS Checksum=%d,…` block (`0x0055c048`, a single 19-field format string) is
  `sprintf`-ed at `0x0048d3d9` with `[0x00584604]` as its first argument. **Unknown:** where that
  buffer goes.
- **Observed.** A second registered procedure (`0x005843f8`/`0x005843fc`) *is* invoked, from
  `0x004b48e0`, but only while `[0x00584418] == 2` (one of the network setup screens) and no more
  than once per 100 ms. That is a lobby tick, not a logger.

### The honest recipe

There is no recipe for the stat log. What *can* be done, with its evidence:

1. **Observed.** `gschecksumon` / `gschecksumoff` (`0x004d6ce0` / `0x004d6cf0`) set and clear
   `0x00584600`, and that is the gate on the `'GS' files` accumulation and nothing else. A mod or
   the console can widen the accumulated range. Useful for *changing* the checksum, not for seeing
   it.
2. **Observed.** `getgameseed` (`0x004e56f0`) returns `[0x00573428]`, the value compared as
   `'Random-Seed'`. A script can print it. Two peers reading out different seeds is a desync you can
   see without any engine logging.
3. **Observed.** `setchecksumproc`'s procedure result is compared as `'Player-Stats'`, and a script
   can compute and display the same value it hands the engine. The vanilla procedure in
   `gs/network.gs` is a worked example.

Those three are script-side and need no patching. Getting the engine's own dump out needs one byte
of the executable changed, which is outside this document's scope and outside its rules.

## 8. `/testseed=` and the rest of the command line

**Observed in a local binary**, `0x004fee60`–`0x004fefb0`. The parser is `strstr` against a literal,
then `atoi` for the `=` forms. The complete set:

| Switch | Effect | At |
| --- | --- | ---: |
| `/s=` | `atoi` into a config field | `0x004fee2x` |
| `/x=` | `atoi` into config `+0x6ac` | `0x004feeaf` |
| `/testseed=` | `atoi` into the global `0x005d2ca4` | `0x004feef8` |
| `/debug` | config `+0x6b0` = 1 | `0x004fef1a` |
| `/nodebug` | config `+0x6b0` = 0 | `0x004fef4e` |
| `/nompq` | config `+0x12c` = 0 | `0x004fef66` |
| `/notrimlogs=` | config `+0x130` = 0 (the value is parsed by nothing; presence is enough) | `0x004fef7e` |
| `/cd=` | string copy | `0x004fef84`+ |
| `/%` | config `+0x6b4` = 0 | `0x004fef36` |

**Unknown:** what reads config `+0x130`, so what `/notrimlogs=` actually changes is not
established. The name is suggestive and the name is all I have.

**Observed.** `0x005d2ca4` is read at exactly three sites and they make `/testseed=` a determinism
switch, not just a seed:

- `0x0048417b`: the host builds message type `0x4c` and sets the game seed to `[0x005d2ca4]` if it
  is non-zero, **otherwise to `rand()`** (`call 0x0053a110` at `0x00484184`), storing it to
  `0x005843e0`.
- `0x0045c722` and `0x0045cdf4`: in two message-construction paths, `[0x005d2ca4] + [0x00573428]` is
  used **in place of `GetTickCount() + [0x00573428]`** when the switch is set.

**Inferred:** `/testseed=<n>` fixes the shared seed *and* replaces the wall clock in two paths that
put a timestamp on the wire. That is the developers' desync-reproduction switch, and it is the right
tool for the two-machine experiment below.

## 9. The pre-flight checker

`spikes/asset-viewer/examples/preflight.rs`, over `install_checksum`. It reports, per pair of
installs: the reproduced `'EXE' version`; exact content comparison of `lomse.exe`, `gs.mpq`,
`imp.mpq` and `pic.mpq`; and a per-member comparison of the script archive with the engine's
accumulator run over every member.

It is deliberately conservative and deliberately limited, and says so in its own output:
`'EXE' version` is a **prediction** about the engine because the algorithm is fully reproduced;
`'GS' files` is a **comparison** because the member set the engine loads is unknown; `'IMP' files`
is a **content statement** because no algorithm exists to reproduce.

### Validated against three installs on this machine

Three installs of the same game, independently confirmed by `shasum` to have byte-identical
`lomse.exe` and `imp.mpq` and three different `gs.mpq`. That makes the checker's output falsifiable:
`'EXE' version` must match for all three pairs, `imp.mpq` must be identical for all three, and the
script content must differ for all three.

| | `'EXE' version` | `imp.mpq` | script content | accumulator |
| --- | --- | --- | --- | --- |
| Steambuild baseline | `149429203` | — | 1687 members, 4 934 695 B | `457423125` |
| 3.02 | `149429203` | — | 1690 members, 5 109 799 B | `470619348` |
| GS5R3 | `149429203` | — | 1699 members, 10 060 912 B | `867588804` |

| Pair | `'EXE' version` | `imp.mpq` | `'GS' files` |
| --- | --- | --- | --- |
| baseline vs 3.02 | will not diverge | identical | **differs** — 379 named members, `457423125` vs `470619348` |
| baseline vs GS5R3 | will not diverge | identical | **differs** — 2406 named members, `457423125` vs `867588804` |
| 3.02 vs GS5R3 | will not diverge | identical | **differs** — 2770 named members, `470619348` vs `867588804` |

All nine predictions hold. `pic.mpq`, which has no divergence class, is identical for the first pair
and differs for the other two.

**The corollary matters more than the tool.** Since the executable is byte-identical across all
three, **`'EXE' version` cannot be what makes a modded install incompatible.** Script content is the
whole story — which is what the first half concluded from the divergence classes, now with the
mechanism and a measurement behind it.

**Caveat, stated because it is the checker's real limit.** The archive comparison covers every
member, not only those the engine loads, so it can report a difference the engine would never see.
That is a false alarm, which is the safe direction; it cannot miss a difference the engine would
see. `(listfile)` is excluded because its contents are the member *names*, which StormLib
synthesises for unnamed members, and two archives naming their unnamed members differently is not a
script difference.

**Caveat on the test.** `install_checksum`'s three-install test skips when the installs are absent,
so on a machine without the game it cannot fail — a real weakness by this repo's standards. The
algorithms are therefore also covered by synthetic tests that always run, and the two were
mutation-checked separately: swapping the sign-extension for a zero-extension fails two synthetic
tests **and passes the three-install test**, which is precisely the limit of what a separation test
can prove.

## What still needs two machines, after the second pass

Items 1–7 of the first list stand. These replace item 8 and add to it.

9. **Run a deliberately reproducible session.** Launch both peers with the *same* `/testseed=<n>`.
   Predicted: identical `gameseed`, and the two paths at `0x0045c722`/`0x0045cdf4` stop contributing
   wall-clock variation. If a desync still occurs, it is reproducible, and that is the difference
   between a bug report and a shrug. This is the experiment design the rest of the list needs.

10. **Read the seed out of both peers with a script.** `getgameseed` is callable from the console.
    Two different values is a `'Random-Seed'` divergence you can see without engine logging. Ten
    seconds of work, and it distinguishes "the seeds never matched" from "the simulations drifted".

11. **Compute `setchecksumproc`'s own procedure on both peers and display it.** Vanilla
    `gs/network.gs` already contains the procedure; running it and printing the result is the only
    way to observe `'Player-Stats'` in this build. If the two agree while the game visibly disagrees,
    the divergence is in something the script checksum does not cover.

12. **Try a session with five or more computers.** The comparator checks at most four
    (`0x004b4f43`). Predicted: peers five and up are never validated against anyone, so a desync on
    those peers is silent. This is Observed code with an Unknown consequence and only a real session
    can say which.

13. **Provoke a `gm_type` 97 (`XFER_PROGRESS`) report.** A joining peer receiving a scenario file is
    the path that sends it. If the reporter is reached with that type it formats a wild pointer.
    Whether that is a crash, garbage, or unreachable in practice is **Unknown** and one join with a
    large scenario would tell.

---

# Third pass: resync is dead, but the engine already tells you

## 10. Resync is unreachable. Nothing ever sends it.

Shift-corrected first, because the slot-versus-type trap has now bitten twice on this branch: table
slots 62 and 63 hold `RESYNC_REQUEST` and `RESYNC_START`, so the real `gm_type`s are **63** and
**64**.

**Observed in a local binary.** There are exactly two message-header builders in the image, and
they are siblings 48 bytes apart:

- `0x0048cc70` takes the type as an argument (`mov al,[esp+0Ch]` / `mov [esi+4],al` at
  `0x0048cc83`/`0x0048cc8a`);
- `0x0048cc40` is the same function with the type hardcoded to zero (`xor al,al` /
  `mov [esi+4],al` at `0x0048cc54`/`0x0048cc5d`), i.e. it can only make a `NO_MESSAGE`.

**Observed.** `0x0048cc70` has **54 call sites**, and every one of them passes a literal
`push imm8` within 26 bytes of the call — there is no site that passes a computed type. Those 54
sites construct **45 distinct message types**. Resolving them against the name table
(shift-corrected) gives the complete set of messages this executable can ever build:

```
4 SWITCH_UNITS        5 SWITCH_COMMANDERS   6 SET_UNIT_COMMANDER  13 MOVE_ARMY (x2)
16 STOP_ARMY          27 ADD_SPELL          28 TRANSFER_ARTIFACT  29 TELEPORT_ARTIFACTS
30 WIELD_ARTIFACT     31 XFER               33 SET_PLAYER         49 START_COMBAT
54 SET_CITY_DATA      55 SET_CITY_NAME      56 SET_BUILDING_DATA  59 ARMY_ANIM
60 ARMY_KILLUNIT      61 TIME_ESTIMATE      62 READY              65 GO (x2)
66 TICK_TIME          68 ORDERS (x8)        69 BATCH_ORDERS       71 SET_LEADER
72 REQUEST_LEADER     73 ASSIGN_LEADER      75 GAME_SETUP         76 START_GAME
77 SET_PLAYER_NAME    78 JUST_ENTERED       79 SAVED_GUIDS        80 PULSE
81 COMPUTERS_ASSIGNED 82 GAME_STARTED       86 REENTER            87 NETMERGE
88 ENTER_BUILDING     90 EVENT_ALARM_NOTIFICATION                 91 CHAMPION_BRAIN_NOTIFICATION
92 SCRIPTCALLBACK     93 REQUEST_SCENARIO   94 REQUEST_START_GAME 95 CHECKSUM
96 AUTOPLAY           97 XFER_PROGRESS
```

**`RESYNC_REQUEST` (63) and `RESYNC_START` (64) are not in that list.** Neither is
`RESYNC_*` reachable through the zero-type builder.

The enum slot and the routing tables exist for them, which is what made them look live:

- **Observed.** The send fan-out dispatcher at `0x00489c6c` (`mov al,[ebp+4]` / `dec eax` /
  `cmp eax,60h` / `mov dl,[eax+489E74h]` / `jmp [edx*4+489E20h]`) has a policy entry for both: they
  share policy `0x00489d0d` with `READY`, `GO` and `TICK_TIME`, which loops `1..numcomputers` and
  sends to **every computer including the sender**.
- **Observed.** The 31..65 gate at `0x00489ad5` (`add ecx,0FFFFFFE1h` / `cmp ecx,22h`) routes both
  to `0x00489aec`, the same branch as 28 other types.

**So the routing is wired and the messages are never built.** That is the same shape of finding as
the disabled post-mortem: a facility present in the tables and unreachable in the code.

**Inferred:** resync was designed, the plumbing survived, and the trigger was never written or was
removed. **Unknown:** what it would have transferred, because there is no builder whose payload
could be read.

## 11. What the file-transfer path can ship — and it is not scripts

This matters because it is what a resync would have had to use, and it decides whether an
always-on host could ever have brought a mismatched peer into line.

**Observed in a local binary.** `XFER` (type 31) is built once, at `0x004b7439`, inside the sender
at `0x004b7410`. The sender:

1. resolves a **file-kind tag** through `0x004b9570` (`0x004b742c`);
2. `CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, 0x08000000, NULL)`
   (`0x004b7468`), `CreateFileMappingA(… PAGE_READONLY …)` (`0x004b748f`), `GetFileSize`
   (`0x004b74b0`), `MapViewOfFile` (`0x004b74c2`);
3. ships the mapping in chunks of **`mov edi,78h` = 120 bytes** (`0x004b74da`, rewritten at
   `0x004b7523`), which sits comfortably inside the 257-byte payload cap from
   [3b](#3b-network-messages-are-capped-at-257-bytes-on-a-fixed-buffer);
4. computes a percentage as `chunk * 100 / total` (`lea eax,[ebx+ebx*4]` / `lea eax,[eax+eax*4]` /
   `shl eax,2` / `idiv` at `0x004b75a8`–`0x004b75b7`) for the `XFER_PROGRESS` message.

**Observed.** The sender has four callers, each passing a literal tag: `0x0045cd7d` (tag 83),
`0x0048286e` (71), `0x00483f95` (77), `0x004a868c` (68). `0x004b9570` maps tags `68..83` through an
index table at `0x004b95f4` and a jump table at `0x004b95e0` onto the path builder `0x00505110`:

| Tag | Resolves to |
| ---: | --- |
| 68 | path kind 7 — `LOMD%.4d.TMP` |
| 71 | path kind 2 — `LOMXFERG.TMP` |
| 77 | the script's `setscenarionameproc` procedure, via `0x004b54c0` — i.e. `map/<name>` or `multisav/<name>` |
| 83 | path kind 3 — `LOMXFERS.TMP` |
| all others in 69..82 | path kind 4 — `LOMXFERU.TMP` |

**Observed.** The path builder `0x00505110` bounds its kind to `0..7` and yields exactly:

| Kind | Path |
| ---: | --- |
| 0 | `savegame/lastsave.lom`, or `multisav/lastsave.lom` when `[0x005851a0]` is set |
| 1 | `LOMGSOUT.TMP` |
| 2 | `LOMXFERG.TMP` |
| 3 | `LOMXFERS.TMP` |
| 4 | `LOMXFERU.TMP` |
| 5 | `APPLOG.TXT` |
| 6 | `LOM_TSPR.TMP` |
| 7 | `LOMD%.4d.TMP` |

**There is no path template for an MPQ archive, and no way for a caller to supply an arbitrary
filename** — the tag is a literal at every call site and the resolver's range is eight fixed kinds
plus the scenario-name procedure.

**So the decisive question has a clean answer: a resync could not have repaired a `'GS' files`
mismatch even if it existed.** The transfer path can move a savegame, a scenario and a handful of
temporaries — game state — and cannot move `gs.mpq`. **The homelab verdict in
[question 4](#4-what-a-homelab-could-and-could-not-do) stands unchanged**, and now for a
structural reason rather than for want of evidence.

## 12. The engine already marks incompatible games in the list

This is the one positive result of the third pass, and it is what a person can act on today.

**Observed in a local binary.** `0x00505f10` reads `[0x00584604]` (the GameScript content
checksum) and `[0x00584424]` (the executable byte sum) and formats them through
`cksum=%d,%d` (`0x005736ec`) at `0x00505f30`. The argument order, traced through both frames, puts
the **script checksum first** and the executable sum second.

**Observed.** Both concrete transports build that tag while setting the session name: the Storm
class at `0x0046fbf8`, inside its vtable slot 2 (`0x0046fbd0`, which copies 29 bytes of the caller's
name into `[this+0xe0]` and NUL-terminates at `+0xfd`); and `CDPlay` at `0x0044a84e`.

**Observed.** `CDPlay` vtable slot 11 (`0x0044a840`, reached by no operator and with zero direct
callers, so virtual-only) calls `0x00505f10`, then compares the local tag against another string
byte by byte (`0x0044a857`–`0x0044a87f`). On **mismatch** it formats the session's display name
through `"*%s"` (`0x00556b24`, pushed at `0x0044a891`); on **match** it copies the name plainly
(`0x0044a8a1`).

**Inferred:** a game in the multiplayer list whose host's build does not match yours is displayed
with a **leading asterisk**, and one that matches is not. It marks; it does not refuse.

That is a shipped, user-facing pre-flight check over exactly the two values `preflight` reproduces.
It also **partly answers the round-two Unknown** about whether a mismatch is caught at join: the
*tag* is compared before you join and marked in the list, while the six-value divergence comparison
still only happens once play is under way. **Unknown:** whether any UI actually renders the
asterisk, since I have not seen the list.

`preflight` now prints the same tag for each install. **Only half of it is comparable to the game's
display:** the executable sum is exact, the script half is the accumulator over a member set the
engine may not load, so the tool's first number will likely differ from the game's. What carries
across is the comparison — same tag means agreement on both halves, different tags mean
disagreement on at least one.

### Two incidental finds

- **Observed.** `APPLOG.TXT` is path kind 5, requested from exactly one site, `0x0048447f`. So
  there *is* an application log path in the shipped build. **Unknown:** what is written to it and
  whether that site is reachable. This is a better lead than `GS.LOG` for anyone who wants engine
  output, and it is unexplored.
- **Observed.** `LOMGSOUT.TMP` (kind 1) is requested from `0x004c9929` and `0x004d7119`, both in the
  GameScript modules. **Unknown** likewise.

## 13. The two-machine experiment, specified

This is the handoff. `/testseed=` makes the whole thing worth doing properly, because it removes the
two known sources of nondeterminism a tester cannot otherwise control
([section 8](#8-testseed-and-the-rest-of-the-command-line)).

**Before anything else — the free check.** Run `preflight` over both machines' `English`
directories, or compare the session tags each machine shows. If the tags differ, stop: fix the
install before testing anything else. This is now the recommended first step for the community and
it needs no session at all.

### Setup, both machines

1. Copy `lomse.exe`, `gs.mpq`, `imp.mpq` and `pic.mpq` from **one** machine to the other, so they
   are byte-identical. Confirm with `preflight` — it must report *will not diverge* on
   `'EXE' version` and *identical* on every file.
2. Pick a `.scn` with all eight faiths, or the multiplayer list will not offer it
   ([3i](#3i-player-and-computer-caps-and-the-map-restriction)).
3. Set `COMBAT_MODE` to `Always Autocalc Combat` for run A and `Observe All Combat` for run B
   ([3g](#3g-there-is-a-real-time-turn-clock-and-a-real-time-combat-mode)).
4. Put both machines on one **layer-2** broadcast domain. Routed-only will not enumerate
   ([question 2](#how-a-session-is-found-and-whether-you-can-type-an-address)).

### Launch

Both machines, same integer, non-zero:

```
lomse.exe /testseed=12345
```

**Observed** why this is the right switch: `0x0048417b` makes the host's game seed `[0x005d2ca4]`
when non-zero and `rand()` otherwise, and `0x0045c722` / `0x0045cdf4` substitute it for
`GetTickCount()` in two message-construction paths. **Inferred:** with it set, two runs of the same
inputs should produce the same simulation, so a desync becomes reproducible.

Add `/debug` on both if you want whatever the debug flag at `[cfg+0x6b0]` enables — **Unknown** what
that is, so treat it as an experiment rather than a step.

### What to capture

The engine's own state dump is unavailable
([section 7](#7-the-desync-post-mortem-is-wired-up-and-disabled)), so capture from the script side:

- **Every turn, on both machines:** `getgameseed` through the console (`~`). Two different values is
  a `'Random-Seed'` divergence and the simulations never agreed.
- **Every turn, on both machines:** run the procedure vanilla `gs/network.gs` hands
  `setchecksumproc` and print the result. That is the `'Player-Stats'` value and the only way to see
  it in this build.
- **The session tag in the game list**, before joining, from both sides.
- A screen recording, because `'PlayAnimation Count'` is a divergence class and animation
  divergence is otherwise invisible.

### What each result would mean

| Observation | Reading |
| --- | --- |
| Session tags differ | Builds differ. Nothing else is worth testing until fixed. |
| Tags match, `getgameseed` differs between machines | The seed exchange failed. `/testseed=` should make this impossible; if it still happens, `START_GAME` (type 76) is not arriving. |
| Seeds match, script checksums diverge at a specific turn | A genuine state divergence. Note the turn — with a fixed seed it should reproduce. |
| Script checksums match while the games visibly disagree | The divergence is in something the script checksum does not cover — the animation counter is the prime suspect. |
| Run A (autocalc) survives and run B (observe all) desyncs | Real-time combat replication is the source. That is the single most useful result on this list, because it is advice a player can follow. |
| Both runs desync identically at the same turn with a fixed seed | Reproducible. That is a bug report someone could act on. |
| Both runs desync at *different* turns with the same `/testseed=` | Something outside the seed is nondeterministic. `/testseed=` covers two paths only, and the rest of the engine's `GetTickCount` use is untouched. |

### On the test-coverage gap

The three-install validation in `install_checksum` skips when the installs are absent, so on a bare
machine it cannot fail. That gap is covered, not open: the property a guessed hash would have
violated — that the two byte sums genuinely differ, because one sign-extends and the other
zero-extends — is asserted by `the_two_checksums_disagree_on_a_high_byte`, which always runs and
which was confirmed to fail when the sign extension is swapped out. The always-running tests pin
the algorithms; the install test only demonstrates separation, and swapping the sign extension
passes it. Both facts are stated in the module documentation.
