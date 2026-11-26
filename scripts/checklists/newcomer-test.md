# Newcomer test kit

Issue [#505](https://github.com/ponkoengine/ponko/issues/505). The agent acceptance suite measures whether an agent can do real tasks with the engine. This kit measures the same for a person who has never seen it, so the guide's gaps show up where a reader meets them.

Read [the conventions](README.md#conventions) first. This page has two parts: what **the person running the test** (the observer) does, and what **the newcomer** is given.

## Who and how many

- At least **five** people who have not used the engine, between them on Windows, macOS and Linux.
- Two of them also do the 3D tutorial. Two come from another engine and use the chapter for it ("Coming from Bevy", `book/src/from-bevy.md`; Unity users start with `book/src/unity.md`).
- Repeat before each minor release with at least two **new** people.

## The observer

1. Give the newcomer only the [task sheet](#task-sheet-give-this-to-the-newcomer) and the guide's address: <https://ponko.dev/guide/>. No help, no hints. If they are stuck for five minutes, write down where and what they tried, then give the smallest hint that unblocks them and mark the line "hint".
2. Record the screen (with their consent), or take notes while they work. Keep nothing that identifies them: use a number (N1, N2, ...) in every record.
3. Start the clock when they open the guide. Fill the [timing sheet](#timing-sheet) as they pass each point.
4. When they finish, ask the [questions](#questions-after-the-test).
5. Turn each stop into a fix to the guide, to an error message or to the API, or into an issue (see [after the test](#after-the-test)).

## Task sheet (give this to the newcomer)

Start with the guide at <https://ponko.dev/guide/>. Work alone. Say aloud (or write down) whenever you are stuck, confused, or searching for something you cannot find.

1. **Install.** Get what you need to build a Rust game on your computer, following the guide.
2. **Getting started.** Follow "Getting started" until a window with your own game opens (`cargo run`).
3. **The 2D tutorial.** Finish "Tutorial: a first 2D game".
4. **One change of your own.** Change something the tutorial did not tell you to change: a new enemy, a different rule, a new sound, a menu. You decide what; the guide and the compiler are your help.
5. **Package it.** Make the download players would get with `ponko package`, unpack it somewhere else and start it.
6. *(Two people only)* **The 3D tutorial.** Finish "Tutorial: a first 3D game".
7. *(Two people only, from another engine)* **Use the "Coming from" chapter** to do step 4 again, and say what in it helped and what it got wrong.

## Timing sheet

One copy per person. Times are minutes:seconds since they opened the guide; the observer fills it.

```text
Person: N__     Date: ______     Revision / guide version: ______
OS and version: ______     Rust installed before? yes / no
Came from another engine? none / Bevy / Unity / other: ______
Did the 3D tutorial? yes / no
```

| Point | Time reached | Hints given | Notes |
|---|---|---|---|
| Rust and tools installed | | | |
| First window (`cargo run` of their own project) | | | |
| 2D tutorial finished | | | |
| Own change finished | | | |
| `ponko package` made a download | | | |
| The download started on its own | | | |
| 3D tutorial finished (if done) | | | |

Every place they stopped (what they were doing, how long, what unblocked them):

| # | Where (chapter, step) | What happened | Minutes lost | What unblocked them |
|---|---|---|---|---|
| 1 | | | | |
| 2 | | | | |
| 3 | | | | |

Every error message they did not understand (paste it exactly):

| # | Message | What they thought it meant | What it meant |
|---|---|---|---|
| 1 | | | |

Everything they searched for and did not find (what words they used, where they looked):

| # | Looking for | Where they looked | Found? |
|---|---|---|---|
| 1 | | | |

## Questions after the test

Ask them, and write down the answers in their words:

1. What was the first thing that made you want to quit, or almost did?
2. Which sentence in the guide did you read twice?
3. What did you expect to find that was not there?
4. Which error message was useless? What should it have said?
5. What did you do that the guide did not say to do?
6. How long did you think the 2D tutorial would take? How long did it take?
7. Would you start a game with this engine? What would stop you?
8. (From another engine) What did you try first because the other engine does it that way? Did the engine do it, and was it easy to find?

## After the test

For each stop, error message and failed search, choose one and write it in the record:

- a **guide fix** (a pull request that changes the page they were on);
- an **error message fix** (the message names the misuse and the fix, as in `book/src/errors.md`);
- an **API change** (an issue first: one obvious way to do each thing);
- an **issue** when it is bigger than that.

## Where to report

Paste the filled timing sheet, the answers and the list of fixes as ONE comment on [#505](https://github.com/ponkoengine/ponko/issues/505) per person (N1, N2, ...); attach the recording only if the newcomer agrees to it being public, otherwise keep it and say so. File each fix as an issue and put its number in the comment. The release tracker counts the people done.
