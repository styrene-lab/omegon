import { Type } from "@sinclair/typebox";
import type { Static } from "@sinclair/typebox";
import type { ExtensionAPI, ExtensionCommandContext, ExtensionContext, RegisteredCommand, ToolDefinition } from "@styrene-lab/pi-coding-agent";

export const SIDE_EFFECT_CLASSES = [
  "read",
  "workspace-write",
  "git-write",
  "external-side-effect",
] as const;

export type SlashCommandSideEffectClass = typeof SIDE_EFFECT_CLASSES[number];

export interface SlashCommandBridgeMetadata {
  agentCallable: boolean;
  sideEffectClass: SlashCommandSideEffectClass;
  requiresConfirmation?: boolean;
  resultContract?: string;
  summary?: string;
}

export interface SlashCommandBridgeEffects {
  filesChanged?: string[];
  branchesCreated?: string[];
  lifecycleTouched?: string[];
  sideEffectClass: SlashCommandSideEffectClass;
}

export interface SlashCommandBridgeNextStep {
  label: string;
  command?: string;
  rationale?: string;
}

export interface SlashCommandBridgeResult<TData = unknown, TLifecycle = unknown> {
  command: string;
  args: string[];
  ok: boolean;
  summary: string;
  humanText: string;
  data?: TData;
  lifecycle?: TLifecycle;
  effects: SlashCommandBridgeEffects;
  nextSteps?: SlashCommandBridgeNextStep[];
  confirmationRequired?: boolean;
}

export type SlashCommandExecutionContext = (ExtensionContext | ExtensionCommandContext) & {
  bridgeInvocation?: boolean;
};

export interface SlashCommandStructuredExecutor<TData = unknown, TLifecycle = unknown> {
  /**
   * Structured executors should return the authoritative result for the bridged operation.
   * If a command intentionally prepares later follow-up work instead of finishing in-band,
   * the returned data should say so explicitly rather than implying completion.
   */
  (args: string, ctx: SlashCommandExecutionContext): Promise<SlashCommandBridgeResult<TData, TLifecycle>>;
}

export interface BridgedSlashCommand<TData = unknown, TLifecycle = unknown> extends Omit<RegisteredCommand, "handler"> {
  structuredExecutor: SlashCommandStructuredExecutor<TData, TLifecycle>;
  bridge: SlashCommandBridgeMetadata;
  interactiveHandler?: (result: SlashCommandBridgeResult<TData, TLifecycle>, args: string, ctx: ExtensionCommandContext) => Promise<void>;
  agentHandler?: (result: SlashCommandBridgeResult<TData, TLifecycle>, args: string, ctx: SlashCommandExecutionContext) => Promise<void>;
}

export interface SlashCommandBridgeExecuteRequest {
  command: string;
  args?: string[];
  confirmed?: boolean;
}

const EXECUTE_PARAMS = Type.Object({
  command: Type.String({ description: "Slash command name without the leading /" }),
  args: Type.Optional(Type.Array(Type.String(), { description: "Command arguments already tokenized" })),
  confirmed: Type.Optional(Type.Boolean({ description: "Confirmation for commands marked requiresConfirmation" })),
});

export type SlashCommandBridgeExecuteParams = Static<typeof EXECUTE_PARAMS>;

function toArgString(args: readonly string[] | undefined): string {
  // Preserve token boundaries for bridged execution (including args containing spaces).
  // Structured executors in bridge mode can parse this JSON payload deterministically.
  return JSON.stringify([...(args ?? [])]);
}

function summarize(command: string, args: readonly string[] | undefined): string {
  const suffix = (args ?? []).length > 0 ? ` ${(args ?? []).join(" ")}` : "";
  return `/${command}${suffix}`;
}

function refusalResult(
  command: BridgedSlashCommand | undefined,
  request: SlashCommandBridgeExecuteRequest,
  summaryText: string,
  humanText: string,
  options?: { confirmationRequired?: boolean },
): SlashCommandBridgeResult {
  return {
    command: request.command,
    args: request.args ?? [],
    ok: false,
    summary: summaryText,
    humanText,
    confirmationRequired: options?.confirmationRequired,
    effects: {
      sideEffectClass: command?.bridge.sideEffectClass ?? "read",
    },
  };
}

export class SlashCommandBridge {
  private readonly commands = new Map<string, BridgedSlashCommand>();

  register<TData>(pi: ExtensionAPI, command: BridgedSlashCommand<TData>): void {
    this.commands.set(command.name, command as BridgedSlashCommand);
    pi.registerCommand(command.name, {
      description: command.description,
      getArgumentCompletions: command.getArgumentCompletions,
      bridge: command.bridge,
      structuredExecutor: command.structuredExecutor,
      handler: async (args, ctx) => {
        const result = await command.structuredExecutor(args, ctx);
        if (command.interactiveHandler) {
          await command.interactiveHandler(result, args, ctx);
          return;
        }
        ctx.ui.notify(result.summary, result.ok ? "info" : "warning");
      },
    });
  }

  get(name: string): BridgedSlashCommand | undefined {
    return this.commands.get(name);
  }

  list(): { name: string; bridge: SlashCommandBridgeMetadata }[] {
    return Array.from(this.commands.values()).map((command) => ({
      name: command.name,
      bridge: command.bridge,
    }));
  }

  async execute(request: SlashCommandBridgeExecuteRequest, ctx: SlashCommandExecutionContext): Promise<SlashCommandBridgeResult> {
    const command = this.commands.get(request.command);
    if (!command) {
      return refusalResult(
        undefined,
        request,
        `/${request.command} is not approved for agent invocation`,
        `Command /${request.command} is not registered with the slash-command bridge.`,
      );
    }

    if (!command.bridge.agentCallable) {
      return refusalResult(
        command,
        request,
        `/${request.command} is not approved for agent invocation`,
        `Command /${request.command} exists but is not allowlisted for agent invocation.`,
      );
    }

    if (command.bridge.requiresConfirmation && !request.confirmed) {
      return refusalResult(
        command,
        request,
        `/${request.command} requires operator confirmation`,
        `Command /${request.command} requires operator confirmation before execution because it may cause ${command.bridge.sideEffectClass} effects.`,
        { confirmationRequired: true },
      );
    }

    const result = await command.structuredExecutor(toArgString(request.args), {
      ...ctx,
      bridgeInvocation: true,
    });
    return {
      ...result,
      command: result.command || command.name,
      args: result.args.length > 0 ? result.args : (request.args ?? []),
      effects: {
        ...result.effects,
        sideEffectClass: result.effects.sideEffectClass ?? command.bridge.sideEffectClass,
      },
    };
  }

  createToolDefinition(name = "execute_slash_command"): ToolDefinition<typeof EXECUTE_PARAMS, { result: SlashCommandBridgeResult; availableCommands: { name: string; bridge: SlashCommandBridgeMetadata }[] }> {
    const bridge = this;
    return {
      name,
      label: "Slash Command Bridge",
      description: "Execute an allowlisted slash command through its shared structured executor.",
      promptSnippet: "Execute an allowlisted slash command and return its structured result envelope.",
      parameters: EXECUTE_PARAMS,
      async execute(_toolCallId, params, _signal, _onUpdate, ctx) {
        const bridgedCtx = {
          ...ctx,
          bridgeInvocation: true,
        };
        const result = await bridge.execute(params, bridgedCtx);
        const command = bridge.get(params.command);
        if (command?.agentHandler) {
          await command.agentHandler(result, toArgString(params.args), bridgedCtx);
        }
        return {
          content: [{ type: "text", text: result.humanText || result.summary || summarize(result.command, result.args) }],
          details: {
            result,
            availableCommands: bridge.list(),
          },
          isError: !result.ok,
        };
      },
    };
  }
}

export function createSlashCommandBridge(): SlashCommandBridge {
  return new SlashCommandBridge();
}

const SHARED_BRIDGE_SYMBOL = Symbol.for("pi-kit-shared-slash-command-bridge");

/**
 * Get the shared SlashCommandBridge instance across all extensions.
 * Creates one if it doesn't exist yet.
 */
export function getSharedBridge(): SlashCommandBridge {
  const global = globalThis as any;
  if (!global[SHARED_BRIDGE_SYMBOL]) {
    global[SHARED_BRIDGE_SYMBOL] = new SlashCommandBridge();
  }
  return global[SHARED_BRIDGE_SYMBOL] as SlashCommandBridge;
}

export function buildSlashCommandResult<TData = unknown, TLifecycle = unknown>(
  command: string,
  args: readonly string[] | undefined,
  options: Omit<SlashCommandBridgeResult<TData, TLifecycle>, "command" | "args">,
): SlashCommandBridgeResult<TData, TLifecycle> {
  return {
    command,
    args: [...(args ?? [])],
    ...options,
    effects: {
      ...options.effects,
      sideEffectClass: options.effects.sideEffectClass,
    },
  };
}
