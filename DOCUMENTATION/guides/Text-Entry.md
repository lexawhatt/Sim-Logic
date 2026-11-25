# Text entry and modal routing

Text entry is separate from physical actions. Register `WindowControls` as an
Application Resource, call `set_text_input(true)` while an editor owns focus,
and read the `FrameText` System parameter in FrameUpdate. Desktop applies the
request after the logical frame. A newly opened editor starts native collection
on the next frame; opening it does not reinterpret earlier physical keys.

`TextEvent` carries committed UTF-8, paste, IME preedit with optional byte
selection endpoints, logical editing commands and cancellation. The application
owns its document, grapheme segmentation, undo, supported-glyph policy and length
limit. Preedit is temporary and must not be appended to the document. Repeated
native editing keys remain occurrences; physical repeats never invent characters.
Debug formatting of semantic input redacts user text.

The complete text batch is limited to 1024 events and 64 KiB, with at most
16 KiB per event. These are UTF-8 byte counts, including preedit snapshots.
Selection endpoints must be UTF-8 character boundaries; reversed selections are
accepted. Invalid batches reject BeginFrame before time, physical input or
Systems change. Desktop buffer errors stop the runner rather than truncate a
commit. Limits bound payloads, not allocator metadata or native event storage.

## Ordering and ownership

Use `FrameText::ordered()` as the single dispatch stream for a modal editor.
It includes raw physical input, even keys without action bindings, and semantic
text in source order. `ModalKeyboard<Scope>` pairs presses and releases with the
scope that accepted the press. Dispatch only its returned deliveries. Changing
scope returns cancellation releases and quarantines held keys until release;
no buffered key is replayed into gameplay. Repeats and unmatched releases are
suppressed. On raw `InputEvent::FocusLost`, call `focus_lost()` to cancel
owners and forget keys whose releases may never arrive. Pointer capture continues to use its existing API.

Process a mode-changing physical event immediately, then interpret subsequent
semantic events using the resulting scope. Thus open/type/close within one frame
has deterministic behavior for text already being collected. On close, clear
application preedit immediately, disable native entry and ignore subsequent text
until the editor reopens. Do not dispatch the same physical snapshot separately
to gameplay. FixedUpdate precedes FrameUpdate: for modal applications, collect
routed gameplay intents in FrameUpdate and apply those intents in the following
fixed tick. Existing FixedInput-based systems do not become modal automatically.

Focus loss cancels composition and discards all semantic content in that frame;
FrameText contains a single Cancel. Desktop ignores subsequent text while
unfocused. Closing native text collection delivers Cancel on the following
frame. A World transition that skips FrameUpdate discards that frame's text.
FrameText is empty in fixed ticks, Startup and rendering stages. Text is never
retained for fixed-step replay. Store the modal router in World state, or cancel
it explicitly when an application-owned router crosses a World boundary.

## Paste and headless injection

Clipboard access is explicit and application-owned. After obtaining authorized
clipboard text, call `WindowControls::paste_text(String)`: the desktop adapter
queues it for the next frame while text entry is enabled. This API does not
install Ctrl+V or read the clipboard automatically. Applications may use their
platform clipboard service or menu action. Headless hosts inject the same
`TextEvent::Paste` directly.

Use `FrameRequest::new(...).with_text(&events)` for tests. Without markers,
semantic events follow all physical input. To interleave them, place
`InputEvent::Text { index }` in the physical event slice: indices must enumerate
the entire semantic slice exactly once, in ascending order. Markers count toward
the physical event cap as well. Invalid or duplicate markers reject the frame.
Desktop produces this ordering automatically.

## Background file IO

Keep paths, document bytes and save policy in the application. A bounded worker
can use `std::sync::mpsc::sync_channel` for requests and completions:

1. Start one worker outside Systems. Give requests a never-reused job ID, the
   issuing World generation and an owned validated snapshot. Cap snapshot bytes
   before cloning and include queued, worker-owned and completion payloads in
   the total memory budget. A capacity-one queue still permits one queued job
   plus one executing job.
2. From FrameUpdate use `try_send`, never blocking `send`. On Full retain the
   unsaved draft and show busy status; on Disconnected show a worker failure.
   Do not spawn a thread for each save or retry in an unbounded loop.
3. The worker performs file IO on its owned data and returns a bounded result
   (job ID, generation, success or structured failure). The application chooses
   temporary-file, flush, rename and durability policy. Never send raw ECS World
   access, renderer handles or mutable document references across threads.
4. In FrameUpdate drain at most a fixed number of completions with `try_recv`.
   Apply only results matching the current generation and pending job ID. A
   stale completion is discarded; it must not mark a new document as saved.
5. On World replacement mark outstanding jobs cancelled and reject stale results.
   Cancellation is cooperative: a filesystem write already started may still
   finish. Generation filtering prevents stale state changes, not external IO.
   If a save must finish before leaving, the application gates its transition.
6. On shutdown close the request channel and drain/drop the completion receiver
   before joining the worker outside the frame loop. A bounded completion send
   can otherwise block shutdown. Report worker panics and IO failures explicitly;
   do not silently discard unsaved drafts or log note contents.

The runtime does not own note paths, privacy rules, document validation or an
async executor. This pattern keeps file work outside the frame thread and World
state changes on the frame thread.

## Native probe

Run `cargo run --features text --example text_entry` in a session with an IME.
The example displays committed text and current preedit separately; F8 explicitly
inserts fixture paste and F12 exits. It never prints document contents. This is
a transport probe, not a complete editor: its Backspace policy removes one scalar.
Use the shaped-cluster geometry APIs when implementing an editor's caret policy.
The optional `--acceptance` argument checks a fixed external automation sequence;
it is not needed for ordinary manual input.
