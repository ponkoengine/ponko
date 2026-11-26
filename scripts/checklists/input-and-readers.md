# Input methods and screen readers

Issue [#460](https://github.com/ponkoengine/ponko/issues/460): recorded runs of typing through each system's input method and of operating the menus with each system's screen reader. The automatic tests pass; what no test did is drive the real thing. One record per OS for each check is the goal; the guide links them ([book/src/ui-accessibility.md](../../book/src/ui-accessibility.md)).

Read [the conventions](README.md#conventions) first. Do the sections for the OS you have. macOS screen reader records already exist (VoiceOver, an outside program, the engine's own window): see the guide page. What is missing is below.

The sample menu for every check: `cargo run --example ui_menu --features accessibility`.

## What to record, for every run

- A transcript or notes of what was said or shown, as text, and screenshots for the IME checks (the preedit and the candidate window).
- The record line: date, hardware, OS and version, the program (IME or screen reader and its version), result, issue number for each bug.
- Save the files under any name and attach them to the comment (see [where to report](#where-to-report)).

## IME typing (all OSes)

The steps are the same everywhere; only the input method changes. The behaviour they check is the chapter's ([book/src/ui.md](../../book/src/ui.md), "Typing with an input method (IME)").

1. Run the sample menu (or any example with a text input) and click the text field.
2. Switch to the input method and type a Japanese word by its sound: `nihon`.
3. **Look for:** the preedit is drawn at the caret, underlined; the candidate window opens next to the caret (not in a corner of the screen); Space moves through candidates; the part being converted is underlined thicker.
4. Press Enter to pick a candidate: the text is committed to the field and the menu does **not** activate its default button.
5. Type another word and press Escape: the composition is cancelled, the field is as it was, and the screen is **not** closed.
6. Type a word, then click another field or press Tab mid-composition: the composition is dropped and nothing is committed into the next field.
7. Take a screenshot of step 3 with the candidate window open.

### macOS: Kotoeri (Japanese)

- **Run:** System Settings → Keyboard → Input Sources → add Japanese (Hiragana); switch with Ctrl-Space or the menu bar. Then the IME steps above.
- **Record:** date ______ · hardware ______ · macOS ______ · result ______ · issue # ______

### Windows: Microsoft IME

- **Run:** Settings → Time & language → Language & region → Japanese → add the Microsoft IME; switch with Win-Space. Then the IME steps above.
- **Record:** date ______ · hardware ______ · Windows ______ · result ______ · issue # ______

### Linux: ibus with Mozc

- **Run:** install `ibus` and `ibus-mozc` (Debian and Ubuntu: `sudo apt-get install ibus-mozc`), add Mozc under Settings → Keyboard → Input Sources, and start the game from that session; try an X11 session and a Wayland session if you have both. Then the IME steps above.
- **Record:** date ______ · hardware ______ · distribution and session ______ · result ______ · issue # ______

## Screen readers

For each, start the screen reader first, then the sample menu. The steps and the expected sounds are in the guide ("Hearing it yourself", "Windows and Linux" in [ui-accessibility.md](../../book/src/ui-accessibility.md)); the list to record:

1. **Tab** through the menu: what is said for each control (its name, then its help or hint).
2. **Play**: its description, and what is said for the status line after it is pressed ("Started a run for Ann", a polite live region).
3. **Volume**: interact and step the slider: the value is said and goes up one step.
4. **Quit**: only the dialog "Quit the game?" with Yes and No can be reached; whether the reading cursor can leave the dialog while it is open; Escape or No brings the menu back.
5. Whether the screen reader ever says the game "is not responding".

Save the speech as a text transcript (the screen reader's speech history or log), and a short screen recording if you can.

### Windows: NVDA, or Narrator

- **Run:** NVDA (download from nvaccess.org, then Ctrl-Alt-N) or Narrator (Ctrl-Win-Enter). Accessibility Insights for Windows or `inspect.exe` show the tree the adapter gives; paste its output for the Play button.
- **Record:** date ______ · hardware ______ · Windows ______ · screen reader and version ______ · result ______ · issue # ______

### Linux: Orca

- **Run:** `orca` (Super-Alt-S on GNOME); Accerciser shows the tree on the accessibility bus. The adapter starts only while the desktop's accessibility is on, which Orca turns on. For the speech log, any text capture of what it said is fine (Orca's speech history, or its debug log).
- **Record:** date ______ · hardware ______ · distribution and session ______ · Orca version ______ · result ______ · issue # ______

### macOS: VoiceOver (already recorded)

Done on macOS 27 on Apple silicon (three records in the guide). Another record on another macOS version or machine is welcome; use the same five points.
- **Record:** date ______ · hardware ______ · macOS ______ · result ______ · issue # ______

## Where to report

ONE comment on [#460](https://github.com/ponkoengine/ponko/issues/460) per run: the record line, the transcript (attached, or pasted in a collapsed `<details>` block), the screenshots. Open an issue for each bug and put its number on the line. A maintainer links the transcripts from the guide's accessibility and IME sections.
