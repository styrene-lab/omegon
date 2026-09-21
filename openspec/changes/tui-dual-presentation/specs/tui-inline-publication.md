# Incremental inline publication — Delta Spec

## ADDED Requirements

### Requirement: Activity guidance remains outside assistant response flow

Inline running and publication status belongs in the composer frame, not between
published assistant text and its unfinished live tail. Transient status yields
to idle composer context when work ends and is never published as conversation.

#### Scenario: Streaming reply continues beside the composer
Given inline Active or Full with published response text and an unfinished live tail
When a running frame is rendered
Then the live tail follows the published text without an intervening status or helper row
And Working and cancellation guidance appear in the composer frame
And completion removes the running indication

#### Scenario: Completed output is still publishing
Given a completed turn with queued inline output
When a frame is rendered
Then publishing status remains in the composer frame until the queue drains
And it does not become part of the model response

### Requirement: Inline output publishes stable canonical text during streaming

Inline publishes accepted input and stable append-only assistant text from the
existing canonical source while the response is still running. Earlier answer text
must be readable in ordinary terminal scrollback before turn completion. The live
area contains the composer, controls, and only the unfinished text tail. Mutable
tool metadata is held until its outcome is stable. Active uses shared outcome
summaries for finalized contiguous tool runs; Full publishes completed evidence.
Publication state contains a cursor and bounded scratch, not a second transcript.

#### Scenario: Long response remains readable before completion
Given an accepted prompt and a response containing more lines than the live viewport
When the provider pauses after streaming complete lines but before finishing the response
Then those lines already appear in primary terminal history in source order
And the first lines remain readable after subsequent lines arrive
And the editor viewport does not serve as the scrolling answer container

#### Scenario: Partial line grows across deltas
Given a response whose last line or grapheme is unfinished
When another delta extends it
Then only newly stable text is committed to native history
And the remaining tail stays visible without replaying its committed prefix

#### Scenario: Unbroken response makes bounded progress
Given a response much larger than one batch with no newline and mixed-width Unicode
When the provider pauses before response completion
Then stable complete display rows drain to native history through bounded cycles
And publication does not wait for another provider delta to drain eligible text
And an unfinished grapheme does not cause an automatic-publication busy loop

#### Scenario: Assistant continues after tools
Given assistant text followed by completed tool work and more assistant text in the same turn
When that later assistant text streams
Then completed contiguous tool runs publish once using the selected detail policy
And later assistant text reaches scrollback before the entire turn ends
And mutable in-progress tool output is not printed as a completed outcome

#### Scenario: Full detail retains late thinking without rewriting answers
Given inline Full with streamed answer text and thinking evidence that may arrive later
When the response becomes complete
Then completed thinking is appended as labeled evidence after the answer
And answer text is neither withheld for thinking nor replayed at completion

#### Scenario: Response completion flushes only the remaining tail
Given a streamed answer with an already published prefix
When MessageEnd or authoritative terminalization closes it
Then its remaining text is flushed once
And completed output matches the accepted source without duplicate prefixes

#### Scenario: Terminal control content remains data
Given provider or tool text containing raw ESC and OSC sequences split across deltas or batches
When inline publication formats that text
Then existing safe display treatment prevents the content from executing terminal commands
And semantic producer and content provenance remain intact

#### Scenario: Wide characters survive physical terminal insertion
Given streamed rows containing double-width characters and combining marks
When the insertion adapter writes those rows to the real terminal backend
Then covered cells do not emit extra spaces that shift and truncate row content
And all non-whitespace source characters remain present in captured terminal history
And this holds when the terminal is no taller than the inline viewport

#### Scenario: Failed or cancelled turn has a final outcome
Given a running turn with partial response or tool output
When authoritative failure or cancellation finalizes it
Then publication includes available finalized content and its truthful terminal outcome
And the live composer can accept the next turn independently of backlog delivery

#### Scenario: Resume does not flood native history
Given a saved session containing extensive completed history
When a new inline attachment resumes it
Then a bounded session summary is published and the automatic cursor begins at the attachment boundary
And full historical conversation remains available in the shared fullscreen transcript

#### Scenario: Fullscreen-first work becomes inline history
Given a fullscreen-first attachment with completed work since attachment
When the operator switches its base to inline
Then work completed since attachment is published in bounded ordered batches
And history predating attachment is not automatically replayed

### Requirement: Inline assistant output retains Markdown presentation and word boundaries

Inline assistant output uses the shared Markdown presentation vocabulary and
terminal theme. Publication preserves styled spans through native insertion.
Ordinary prose wraps between words when a word fits within the available width;
only oversized tokens require grapheme-safe splitting. Markdown context survives
provider chunks and publication batches. Completed output does not replay earlier
rows merely to apply formatting. Pending output uses the current terminal width;
already published physical rows remain terminal history.

#### Scenario: Styled prose before response completion
Given a streamed answer with a heading, bold prose, inline code, and a list
When the provider pauses after those constructs and before completing the turn
Then native history contains readable formatted content with the corresponding styles
And heading and emphasis delimiters are not printed as literal Markdown syntax
And list continuations retain their indentation
And normal words that fit within a row are not split across rows

#### Scenario: Markdown syntax crosses transport boundaries
Given emphasis delimiters, inline code, and fence markers split across provider chunks
When successive chunks complete those constructs
Then formatting depends on the source document rather than transport chunk boundaries
And committed text is neither lost nor duplicated at completion

#### Scenario: Code and tables retain structure
Given a streamed fenced code block with indentation and a Markdown table
When each structure becomes eligible for publication
Then code indentation and literal code contents remain readable
And table headings and cells use the shared table presentation
And prose wrapping does not collapse code indentation or split table syntax into unrelated paragraphs

#### Scenario: Long table makes progress before its final row
Given a streamed table with a complete header, separator, and body rows
When the provider pauses before ending the table
Then completed rows already appear in native history
And later rows preserve the pinned column layout or use a lossless narrow-width presentation
And table length does not require retaining the entire table in publication scratch

#### Scenario: Resize during styled prose
Given an inline reply partly published at one terminal width
When the terminal changes width while more prose arrives
Then pending prose wraps at the new available width with styles intact
And previously published rows are not replayed to simulate historical reflow

### Requirement: Publication preparation and allocation are bounded

Each loop cycle admits input and authoritative lifecycle events before at most one
publication batch. Discovery, formatting, wrapping, and allocation share explicit
limits. Default maxima are 64 KiB source text, 64 records, 1,000 rendered rows,
65,536 cells, and a cooperative 5 ms preparation slice. No pre-budget full-history
export, hash, clone, or unbounded single-record parse is permitted.

#### Scenario: Persistent notice after published attachment
Given native scrollback already contains the attachment and an earlier system notice
When a control response or local command produces a persistent system notice
Then the new notice is appended once as a separate publication
And earlier printed text is not mutated or repeated

#### Scenario: Mutable plan progress does not block a live answer
Given a mutable plan-progress snapshot followed by streamed assistant text
When automatic inline publication reaches the snapshot
Then it advances past that snapshot and publishes eligible answer text
And the current plan remains available in Workbench, fullscreen history, and explicit export
And immutable notices and lifecycle records still publish in conversation order

#### Scenario: Retained notification history rolls over
Given the retained system-notification limit has been reached and earlier notices were published
When a new notice prunes older retained notifications
Then the new notice remains eligible for publication once
And repeated rollover does not stop future output or replay old output
And retained notification history remains bounded

#### Scenario: Pruning before a partially published retained record
Given a retained record has a committed partial prefix and older notifications precede it
When notification retention prunes those older records
Then publication retains the committed field and byte position in the retained record
And its remaining content is published without repeating the prefix
And stale prepared batches cannot settle against the changed source

#### Scenario: Backlog remains interruptible
Given many completed groups accumulated while fullscreen is open
When inline resumes with a queued decision and cancellation input
Then interaction and lifecycle admission precede publication
And each batch respects every configured limit while subsequent cycles make progress

#### Scenario: A text cluster exceeds the inline allocation limit
Given a single grapheme cannot fit within the bounded inline publication buffer
When publication encounters that cluster
Then automatic publication stops with a specific text-limit notice
And the canonical text remains available through fullscreen history or explicit export
And input and authoritative completion remain responsive
And a new conversation attachment clears this text-limit condition without clearing uncertain-write protection

#### Scenario: Oversized Unicode record at narrow width
Given a single response larger than the byte budget with wide characters, combining marks, and long unbroken lines
When it publishes at a narrow terminal width
Then preparation and insertion allocate within the cell and row limits before calling Ratatui
And successive chunks reproduce the intended text without dropped or duplicated source content
And height never silently truncates through a u16 conversion

#### Scenario: Zero-size viewport
Given a terminal reporting zero width or height
When a publication cycle runs
Then it performs no insertion or cursor advancement
And resize to usable dimensions makes the pending work eligible again

### Requirement: Delivery settlement reflects physical uncertainty

Commit only after verified inline ownership, insertion, and flush succeed.
Known non-writes leave the cursor retryable. A write or flush failure that may have
partially reached the terminal degrades the attachment and disables automatic replay.
The UI retains transcript/export access and reports the limitation persistently.

#### Scenario: Successful repeated frames do not duplicate output
Given a successfully committed publication chunk
When another frame or a fullscreen round trip occurs without newly eligible content
Then the committed chunk is not inserted again

#### Scenario: Fullscreen rejects automatic insertion
Given fullscreen owns the terminal with completed unpublished content
When automatic publication is considered
Then no insert_before call or settlement occurs
And the content remains eligible after a successful return to inline

#### Scenario: Known non-write retries without advancing
Given a preparation or ownership validation failure before any write attempt
When a later relevant event allows the pending chunk to publish
Then the original source range is delivered once and advances only after successful flush

#### Scenario: Partial write is not blindly retried
Given insertion or flush fails after output may have begun
When the event loop runs again or presentation changes
Then automatic publication remains disabled for that attachment
And a persistent delivery-uncertain indication provides managed transcript/export access
And no automatic attachment reset or duplicate replay occurs

### Requirement: Formatting changes and source replacement preserve cursor meaning

Publication cursors belong to an attachment and canonical generation. Width changes
invalidate uncommitted wrapping. Detail changes apply to records not yet started;
a partially committed record finishes at its original detail. Published terminal
history remains immutable. Source replacement invalidates stale prepared content.

#### Scenario: A rewrite invalidates an already streamed cursor
Given automatic output advanced beyond the turn-finalized boundary
When a source rewrite invalidates its cursor without a precise mapping
Then automatic publication pauses with a conversation-changed notice
And it does not restart at the older finalized boundary or replay streamed text
And explicit new attachment can resume publication while uncertain-write protection remains intact
And typed notification pruning continues to preserve exactly mapped cursor positions

#### Scenario: Resize and detail change during a partial record
Given part of a long Full-detail record is committed
When the terminal narrows and detail changes to Active
Then its remaining source is rewrapped at the new width using the same record detail
And later records use Active without replaying the committed prefix

#### Scenario: Canonical replacement rejects stale settlement
Given a prepared chunk from a prior session or pre-compaction generation
When the canonical source is replaced
Then settlement of that chunk is rejected
And printed history is preserved with a bounded boundary notice before new-generation output
And replacement history is not automatically dumped into scrollback

#### Scenario: Explicit export is separate from automatic delivery
Given inline with a committed prefix and pending automatic publication
When the operator invokes /session-export scrollback
Then a labeled explicit snapshot uses serialized terminal ownership
And inline geometry is restored without resetting the automatic cursor
And any intentional snapshot repetition is not reported as automatic exactly-once delivery

#### Scenario: Exit does not wait for the entire backlog
Given pending publication larger than one batch
When the operator exits normally
Then at most one bounded publication slice is drained before cleanup
And remaining history is identified as available in the managed or saved transcript
