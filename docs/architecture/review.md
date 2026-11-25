# Transaction Review

## Who this is for

The person holding the signer. A transaction has arrived on the USB
stick and the interpreter has turned it into fields; before anything is
signed, they read every field on a 240×240 screen, then switch to the
Sign screen with #6 and add a signature with #5 — or finish without one.

!!! note "What runs today"
    The desktop simulator runs the whole flow: USB folder, interpreter,
    this review, signing. On the Raspberry Pi, `signer-pi-demo` runs this
    review on a built-in sample transaction and reports the decision;
    USB, interpreter and secure-element signing on the device are later
    phases. Binding what is shown to the exact bytes signed is tracked in
    [#10](https://github.com/paolino/air-gap-signer/issues/10).

The screen is small and a real transaction is not, so the review is a
tree browsed one level at a time, each level showing its groups' entries
indented under them. The rule that makes it safe: **#5 does
nothing until everything has been shown** — every entry of every group,
and every long value or group name read to its end. A transaction with
no fields at all is refused outright. A transaction cannot be approved
with a field still hidden in an unvisited branch or behind an ellipsis.

## What the user sees

```text
REVIEW          7 left      outputs›_0     3 left      outputs›_0›addr… 1/1
fee: 171485                 address: addr…tjqp          address:
network: preprod      →     lovelace: 5000000     →      addr_test1qz2fxv2um
outputs                                                  yhttkxyxp8x0dlpdt3k
  _0                                                     6cwng5pxj3jhsydzer3
  _1                                                    …
validity_e…: 87654321
```

- The **header** is the branch from the root to the level shown,
  `outputs›_0`; at the top it names the mode, `REVIEW`. Each step right
  adds a segment, each step left removes one. When it no longer fits, its start is cut so the deepest
  segments stay visible. On the right it counts what is still to see,
  and says `done` when nothing is left — or `2 waived` when some of it
  was accepted unread.
- Below it are the **entries** of that level, and under each group its
  own entries, indented. A group whose entries are listed under it is a
  plain heading (`outputs`). A nested group (`_0`) shows just its name
  — no `: value` — and right goes into it. Values are `key: value`. List items are
  numbered `_0`, `_1`, …
- **Colour** says what still blocks approval: a row is light red while
  it, or anything under it, has not been shown yet — an unread cut value,
  a group with fields not yet on screen — and white once it is done. The
  header is amber.
- The cursor is an amber stripe at the right edge of its row, in the
  margin past the last column, so it never covers text or changes its
  colour.
- The cursor stops on fields and on nested groups, never on a heading —
  unless the heading's name is cut and still has to be read.
- A value too long for its row is **cut in the middle** with `…`,
  keeping the key and both ends of the value. A group whose name does
  not fit is cut the same way.
- Opening a value shows the **key on its own rows and the value wrapped
  under it**, a page at a time; the header shows the page. Opening a
  group whose name was cut first shows its full name; right again goes
  into it.
- Every character outside printable ASCII is **spelled out** as
  `\u{hex}`, e.g. `東` as `\u{6771}`, and a backslash as `\\`.
  Characters the font cannot draw, control characters, look-alikes and
  literal escapes therefore never show as each other.
- Nothing shows as blank: an empty string is `""`, a string with spaces
  at either end is quoted (`" x"`), an empty object is `{}`.

## Controls

| Control | In a level | In a value |
|---|---|---|
| joystick up / down | move the cursor to the previous / next field or nested group, skipping headings; a press at the last or first row wraps around; held, it repeats and stops there | previous / next page |
| joystick right | on a nested group: go into it; on a value or a cut name: show it in full; on a heading: nothing | from a group's full name: go into it |
| joystick left | back to the screen you came from, onto the row you left | back to the level |
| button #5 | on a light-red row (a branch or a cut value still to see): accept it unread; on a grey row: undo that | — |
| button #6 | next mode: Review → Sign → Keys | next mode |
| joystick press | — | — |

In the desktop simulator the arrow keys stand in for the joystick,
Enter for #5 and Tab for #6.

## Modes

Button #6 moves between three screens; the header names Sign and Keys,
and Review shows `REVIEW` at the top and its branch below that.

- **Review** — this page: browse, read, accept unread.
- **Sign** — the keys able to sign, then **Done**. #5 on a key adds its
  signature at once (`SIGNED slot 0`); #5 again removes it. There is no
  confirmation and no reject: the toggle is the undo, and Done with no
  signature signs nothing. Adding is refused while anything is left to
  review — the header says how much. Signatures stay in memory until
  Done. Without a key the header says `SIGN no key` and only Done is
  offered.
- **Keys** — PIN, create, restore, back up, export the public key; listed,
  not yet wired to the secure element.

```mermaid
stateDiagram-v2
    [*] --> Review: transaction interpreted
    Review --> Sign: #6
    Sign --> Keys: #6
    Keys --> Review: #6
    Review --> Review: browse, #5 accepts unread or undoes
    Sign --> Sign: #5 on a key adds or removes its signature (adding needs nothing left)
    Sign --> [*]: #5 on Done, with the signatures still added
```

## States of the review

```mermaid
stateDiagram-v2
    [*] --> Level: transaction interpreted
    Level --> Level: up / down moves the cursor
    Level --> Level: right on a group enters it
    Level --> Level: left goes back up
    Level --> Value: right on a value
    Value --> Value: up / down turns the page
    Value --> Level: left
    Level --> Level: #5 on a light-red row accepts it unread, on a grey row undoes that
    Level --> Ready: nothing left to see
    Ready --> [*]: signing allowed in Sign mode
```

## What counts as shown

```mermaid
flowchart LR
    E["entry"] -->|"its row was on screen,<br/>at its level or nested"| S["seen"]
    V["value or group name cut with …"] -->|"shown whole, or opened<br/>and paged to its last page"| R["read"]
    S --> C{"every entry seen<br/>and every cut entry read?"}
    R --> C
    C -->|yes| OK["header says done<br/>Sign mode can sign"]
    C -->|no| N["header counts what is left<br/>signing is refused"]
```

An entry reaches the screen in its parent's level, or nested one level
under its group in the level above; deeper branches must be visited. Opening a long value without
paging to its end does not count.

## Accepting without reading

On a light-red row — a branch with something still to see, or a value
cut with `…` and not read — #5 accepts it unread at once. It and
everything under it stop blocking approval and turn **grey**, apart from
what was actually read; #5 again on the grey row undoes it. When nothing
is left the header says `2 waived` instead of `done`, so approval
means "everything was shown, or knowingly accepted unread". On a row
already read, #5 does nothing.

## Buttons cannot answer a screen they did not see

Before each answer the device waits 100 ms for the panel to show the new
screen, then until every control is released, then one debounce period,
and throws away any press queued meanwhile. A press made before the
screen changed, or held across the change, cannot approve what replaced
it. Two narrower gaps remain — a press starting just before the queue
is cleared, and a panel transfer that silently fails — and are tracked
in [#21](https://github.com/paolino/air-gap-signer/issues/21).

## Decisions

| Chosen | Rejected | Why |
|---|---|---|
| Tree browsed one level per screen with entries nested under their groups, branch in the header | One flat list of every line; one level per screen with groups closed | A real transaction is too deep for a flat list on 11 rows. Closed groups said nothing about what is inside; nesting one level shows it, and drilling down keeps each screen readable while the header says where you are. |
| Cut long values in the middle, keep the key | Wrap every value in the list; cut at the end | Wrapping makes the list unreadable; cutting at the end hides the tail of an address, which is where address-poisoning attacks hide differences. |
| #5 locked until everything was shown | Let the user approve at any time | A field the user never saw cannot have been checked. |
| Joystick right and left to go in and out | Joystick press to open | Tried on the device: pressing the joystick in was awkward. |
| Groups shown by name alone, no marker, no bold | A `›` before each group; groups in bold | Tried on the device: the marker was hard to read and the bold was confusing; a name without `: value` already says it is a group. |
| No counts after nested groups | Counts like `{2}` after nested groups | Tried on the device: counts added noise; the list keys `_0`, `_1` already say it is a list. |
| Light red for rows still to see, white when done, amber header | One colour for everything; a counter only; a cyan header | The counter says how much is left, the colour says where. Tried on the device: cyan read almost white on this panel, so the header lost its colour. |
| Spell out non-ASCII as `\u{hex}` | Draw it with the font | The font lacks most scripts; missing glyphs all look the same, so different values could look identical. |
| List items as `_0`, `_1` | `[0]`, `[1]` | Tried on the device: lighter to read. An object key literally named `_0` would look like a list item; transaction fields are not named that way. |
| Cursor skips headings | Cursor stops on every row | A heading is not a field and right does nothing on it. |
| Cursor as an amber stripe at the right edge | Full-width bars (dark blue, light, amber, colour-inverted, grey), outline box, underline, left stripe, highlighter | Tried on the device and on camera: bars made text unreadable on camera; the stripe keeps every row's text and colour as is. |
| Only up and down repeat when held | Every control repeats | A held button must never confirm, reject, open or go back by itself. |
| #5 accepts a branch or a cut value unread at once, #5 again undoes; grey rows; counted in the header | Only full review; a confirmation question before accepting; values never accepted | Tried on the device: the question was one press too many and the undo serves the same purpose; accepting stays visible (grey rows, `2 waived`). |
| #6 cycles Review, Sign, Keys; #5 adds and removes a signature | #6 rejects; a confirmation before signing; a Reject entry | Tried on the device: a signature that can be removed needs no confirmation, and finishing with none already rejects. |
