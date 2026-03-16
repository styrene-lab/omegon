import type { ExtensionAPI } from "@styrene-lab/pi-coding-agent";

// @config HINTS_CYCLE_INTERVAL_MS "How often to rotate the tip during agent processing (ms)" [default: 12000]

const TIPS = [
	// TUI Commands
	"Type /dash to expand the TUI into a full-screen dashboard — session stats, active tools, and model info at a glance",
	"Type /model to switch the active AI model on the fly — no restart required",
	"Type /fork to branch the current session and explore an alternative approach without losing your place",
	"Type /tree to navigate your session history as a branching tree — every fork is preserved",
	"Type /compact to summarize and compress conversation history, freeing up context space for longer sessions",
	"Type /clear to wipe the chat history and start fresh while keeping your session context",
	"Type /effort to dial the agent's reasoning depth up or down for the current task",
	"Type /bootstrap to check and install all recommended system dependencies in one shot",

	// Keyboard Shortcuts
	"Shift+Enter inserts a newline so you can write multi-line prompts before sending",
	"Ctrl+C cancels the current agent operation mid-flight — useful when a tool call runs long",

	// Skills & Extensions
	"Skills are expert prompt modules that shape how the agent works — /cleave, /opsx, and /assess are built in",
	"Use /opsx:propose to start spec-driven development — the agent writes Given/When/Then specs before touching code",
	"Use /cleave to break a complex task into parallel workstreams the agent can execute simultaneously",
	"The vault extension secures your API keys and credentials using HashiCorp Vault — no plaintext secrets",
	"The web-search extension gives the agent live internet access — just ask it to research anything",
	"The local-inference extension lets the agent switch to an on-device Ollama model for offline or cost-free work",

	// Agent Capabilities
	"The agent can read, edit, and create files across your entire project — ask it to refactor whole directories",
	"Project memory persists facts across sessions — the agent remembers your architecture, decisions, and constraints",
	"The agent can run bash commands, install packages, and interact with your system directly during a session",
	"Extensions live in the extensions/ directory — build your own tools, commands, and agent workflows in TypeScript",
	"Multiple sessions run in parallel branches — fork freely and merge the best result",
	"The agent can query memory_recall to surface relevant facts from dozens of past sessions in milliseconds",
	"Tool profiles let you enable or disable groups of tools per task — keep the agent focused on what matters",
	"Omegon is an agentic harness — every part of it, from skills to extensions, is designed to be customized",
];

function randomTip(): string {
	return TIPS[Math.floor(Math.random() * TIPS.length)];
}

function renderWidget(tip: string, ctx: any): string[] {
	const theme = ctx.ui.theme;
	return [theme.fg("accent", "💡 Hint  ") + theme.fg("dim", tip)];
}

export default function (pi: ExtensionAPI) {
	// One tip on startup — shown as a persistent widget above the editor so it's
	// always visible at the bottom of the screen regardless of init log volume.
	// Cleared when the user sends their first message.
	let startupTipShown = false;

	pi.on("session_start", async (_event, ctx) => {
		ctx.ui.setWidget("hints-startup", renderWidget(randomTip(), ctx));
	});

	pi.on("before_agent_start", async (_event, ctx) => {
		if (!startupTipShown) {
			startupTipShown = true;
			ctx.ui.setWidget("hints-startup", undefined);
		}
	});

	// Cycling tips during agent processing — sits above the editor, below the chat/spinner
	let interval: ReturnType<typeof setInterval> | null = null;

	pi.on("agent_start", async (_event, ctx) => {
		// Show immediately
		ctx.ui.setWidget("hints", renderWidget(randomTip(), ctx));

		// Cycle on interval
		interval = setInterval(() => {
			ctx.ui.setWidget("hints", renderWidget(randomTip(), ctx));
		}, 12_000);
	});

	pi.on("agent_end", async (_event, ctx) => {
		if (interval) {
			clearInterval(interval);
			interval = null;
		}
		ctx.ui.setWidget("hints", undefined);
	});
}
