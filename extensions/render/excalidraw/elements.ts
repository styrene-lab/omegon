/**
 * Excalidraw Element Factories
 *
 * Vendored and merged from @swiftlysingh/excalidraw-cli@1.1.0:
 *   - src/factory/element-factory.ts (base element creation)
 *   - src/factory/node-factory.ts (shape creation)
 *   - src/factory/connection-factory.ts (arrow creation)
 *   - src/factory/text-factory.ts (text creation)
 *
 * Changes from upstream:
 *   - Merged 4 files into 1 for simpler vendoring
 *   - Replaced nanoid with crypto.randomUUID()
 *   - Added semantic palette integration (optional `semantic` param)
 *   - Added document builder (createDocument)
 *   - Added validation (validateDocument)
 *   - Added layout helpers (fanOut, timeline, grid)
 *   - Changed default roughness from 1 to 0 (clean/modern)
 *
 * See UPSTREAM.md for sync guide.
 */

import { randomUUID } from "node:crypto";
import {
	DEFAULT_ELEMENT_STYLE,
	DEFAULT_APP_STATE,
	SEMANTIC_COLORS,
	TEXT_COLORS,
	FONT_FAMILIES,
	type ElementBase,
	type ExcalidrawElement,
	type ExcalidrawFile,
	type RectangleElement,
	type DiamondElement,
	type EllipseElement,
	type TextElement,
	type ArrowElement,
	type LineElement,
	type BoundElement,
	type ArrowBinding,
	type Arrowhead,
	type FillStyle,
	type StrokeStyle,
	type TextAlign,
	type VerticalAlign,
	type Roundness,
	type SemanticPurpose,
	type TextLevel,
} from "./types.ts";

// Re-export types for consumers
export type {
	ExcalidrawElement,
	ExcalidrawFile,
	RectangleElement,
	DiamondElement,
	EllipseElement,
	TextElement,
	ArrowElement,
	LineElement,
	SemanticPurpose,
	TextLevel,
};

// ---------------------------------------------------------------------------
// Internal helpers (from upstream element-factory.ts)
// ---------------------------------------------------------------------------

let indexCounter = 0;

function generateIndex(): string {
	indexCounter++;
	let idx = indexCounter;
	let result = "";
	while (idx > 0) {
		idx--;
		result = String.fromCharCode(97 + (idx % 26)) + result;
		idx = Math.floor(idx / 26);
	}
	return "a" + result;
}

function generateSeed(): number {
	return Math.floor(Math.random() * 2147483647);
}

/** Reset index counter — useful for tests or fresh documents. */
export function resetIndexCounter(): void {
	indexCounter = 0;
}

/** Generate a short element ID. Uses crypto.randomUUID() instead of nanoid. */
function newId(): string {
	return randomUUID().replace(/-/g, "").slice(0, 21);
}

// ---------------------------------------------------------------------------
// Base element factory (from upstream element-factory.ts)
// ---------------------------------------------------------------------------

function createBaseElement(
	type: ElementBase["type"],
	x: number,
	y: number,
	width: number,
	height: number,
	options?: Partial<ElementBase>,
): ElementBase {
	return {
		id: options?.id || newId(),
		type,
		x,
		y,
		width,
		height,
		angle: options?.angle ?? 0,
		strokeColor: options?.strokeColor ?? DEFAULT_ELEMENT_STYLE.strokeColor,
		backgroundColor: options?.backgroundColor ?? DEFAULT_ELEMENT_STYLE.backgroundColor,
		fillStyle: options?.fillStyle ?? DEFAULT_ELEMENT_STYLE.fillStyle,
		strokeWidth: options?.strokeWidth ?? DEFAULT_ELEMENT_STYLE.strokeWidth,
		strokeStyle: options?.strokeStyle ?? DEFAULT_ELEMENT_STYLE.strokeStyle,
		roughness: options?.roughness ?? DEFAULT_ELEMENT_STYLE.roughness,
		opacity: options?.opacity ?? DEFAULT_ELEMENT_STYLE.opacity,
		groupIds: options?.groupIds ?? [],
		frameId: options?.frameId ?? null,
		index: options?.index ?? generateIndex(),
		roundness: options?.roundness ?? null,
		seed: options?.seed ?? generateSeed(),
		version: options?.version ?? 1,
		versionNonce: options?.versionNonce ?? generateSeed(),
		isDeleted: options?.isDeleted ?? false,
		boundElements: options?.boundElements ?? null,
		updated: options?.updated ?? Date.now(),
		link: options?.link ?? null,
		locked: options?.locked ?? false,
	};
}

// ---------------------------------------------------------------------------
// Approximate text measurement (from upstream text-factory.ts)
// ---------------------------------------------------------------------------

const DEFAULT_FONT_SIZE = 16;
const DEFAULT_FONT_FAMILY = FONT_FAMILIES.Cascadia; // 3 — monospace
const DEFAULT_LINE_HEIGHT = 1.25;

function measureTextApprox(
	text: string,
	fontSize: number,
): { width: number; height: number } {
	const lines = text.split("\n");
	const maxLen = Math.max(...lines.map((l) => l.length));
	// Approximate char width for monospace at given font size
	const charWidth = fontSize * 0.6;
	const lineHeight = fontSize * DEFAULT_LINE_HEIGHT;
	return {
		width: maxLen * charWidth,
		height: lines.length * lineHeight,
	};
}

// ---------------------------------------------------------------------------
// Semantic palette helpers (pi-kit addition)
// ---------------------------------------------------------------------------

/**
 * Approximate relative luminance of a hex color (sRGB → linear → ITU-R BT.709).
 * Returns 0.0 (black) to 1.0 (white). Used to pick text color for readability.
 */
function luminance(hex: string): number {
	const h = hex.replace("#", "");
	if (h.length < 6) return 1; // fallback to "light" for invalid/short values
	const r = parseInt(h.slice(0, 2), 16) / 255;
	const g = parseInt(h.slice(2, 4), 16) / 255;
	const b = parseInt(h.slice(4, 6), 16) / 255;
	// sRGB to linear
	const toLinear = (c: number) => c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
	return 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
}

/**
 * Pick a readable text color for the given background.
 * Transparent backgrounds use the stroke color; filled backgrounds
 * use white or dark gray depending on fill luminance.
 */
function textColorForBackground(bg: string, stroke?: string): string {
	if (bg === "transparent") {
		return stroke ?? DEFAULT_ELEMENT_STYLE.strokeColor;
	}
	return luminance(bg) < 0.4 ? TEXT_COLORS.onDark : TEXT_COLORS.onLight;
}

/**
 * Extract only ElementBase-compatible properties from semantic colors.
 * Does NOT spread the full options object — prevents leaking non-ElementBase
 * keys (label, semantic, labelFontSize, etc.) into the returned partial.
 */
function applySemanticColors(
	options: Partial<ElementBase> & Record<string, unknown>,
	semantic?: SemanticPurpose,
): { backgroundColor?: string; strokeColor?: string } {
	const colors = semantic ? SEMANTIC_COLORS[semantic] : undefined;
	return {
		backgroundColor: options.backgroundColor ?? colors?.fill,
		strokeColor: options.strokeColor ?? colors?.stroke,
	};
}

// ===================================================================
// PUBLIC API — Shape Factories
// ===================================================================

export interface ShapeOptions {
	id?: string;
	semantic?: SemanticPurpose;
	label?: string;
	labelFontSize?: number;
	strokeColor?: string;
	backgroundColor?: string;
	fillStyle?: FillStyle;
	strokeWidth?: number;
	strokeStyle?: StrokeStyle;
	roughness?: number;
	groupIds?: string[];
}

/**
 * Create a rectangle element, optionally with centered label text.
 * Returns [rectangle] or [rectangle, textElement] if label is provided.
 */
export function rect(
	x: number, y: number, w: number, h: number,
	opts: ShapeOptions = {},
): ExcalidrawElement[] {
	const colors = applySemanticColors(opts, opts.semantic);
	const id = opts.id || newId();
	const boundElements: BoundElement[] = [];

	const elements: ExcalidrawElement[] = [];

	if (opts.label) {
		const textId = newId();
		boundElements.push({ id: textId, type: "text" });

		const fontSize = opts.labelFontSize ?? DEFAULT_FONT_SIZE;
		const dims = measureTextApprox(opts.label, fontSize);

		elements.push({
			...createBaseElement("text", x + (w - dims.width) / 2, y + (h - dims.height) / 2, dims.width, dims.height, {
				id: textId,
				strokeColor: textColorForBackground(
					colors.backgroundColor ?? DEFAULT_ELEMENT_STYLE.backgroundColor,
					colors.strokeColor,
				),
			}),
			type: "text",
			text: opts.label,
			fontSize,
			fontFamily: DEFAULT_FONT_FAMILY,
			textAlign: "center" as TextAlign,
			verticalAlign: "middle" as VerticalAlign,
			containerId: id,
			originalText: opts.label,
			autoResize: true,
			lineHeight: DEFAULT_LINE_HEIGHT,
		} as TextElement);
	}

	// Rectangle goes first (container before content for binding)
	elements.unshift({
		...createBaseElement("rectangle", x, y, w, h, {
			id,
			roundness: { type: 3 },
			boundElements: boundElements.length > 0 ? boundElements : null,
			fillStyle: opts.fillStyle,
			strokeWidth: opts.strokeWidth,
			strokeStyle: opts.strokeStyle,
			roughness: opts.roughness,
			groupIds: opts.groupIds,
			...colors,
		}),
		type: "rectangle",
	} as RectangleElement);

	return elements;
}

/**
 * Create a diamond element, optionally with centered label text.
 */
export function diamond(
	x: number, y: number, w: number, h: number,
	opts: ShapeOptions = {},
): ExcalidrawElement[] {
	const colors = applySemanticColors(opts, opts.semantic);
	const id = opts.id || newId();
	const boundElements: BoundElement[] = [];
	const elements: ExcalidrawElement[] = [];

	if (opts.label) {
		const textId = newId();
		boundElements.push({ id: textId, type: "text" });
		const fontSize = opts.labelFontSize ?? DEFAULT_FONT_SIZE;
		const dims = measureTextApprox(opts.label, fontSize);
		elements.push({
			...createBaseElement("text", x + (w - dims.width) / 2, y + (h - dims.height) / 2, dims.width, dims.height, {
				id: textId,
				strokeColor: textColorForBackground(
					colors.backgroundColor ?? DEFAULT_ELEMENT_STYLE.backgroundColor,
					colors.strokeColor,
				),
			}),
			type: "text",
			text: opts.label, fontSize, fontFamily: DEFAULT_FONT_FAMILY,
			textAlign: "center" as TextAlign, verticalAlign: "middle" as VerticalAlign,
			containerId: id, originalText: opts.label, autoResize: true, lineHeight: DEFAULT_LINE_HEIGHT,
		} as TextElement);
	}

	elements.unshift({
		...createBaseElement("diamond", x, y, w, h, {
			id,
			roundness: { type: 2 },
			boundElements: boundElements.length > 0 ? boundElements : null,
			fillStyle: opts.fillStyle,
			strokeWidth: opts.strokeWidth,
			strokeStyle: opts.strokeStyle,
			roughness: opts.roughness,
			groupIds: opts.groupIds,
			...colors,
		}),
		type: "diamond",
	} as DiamondElement);

	return elements;
}

/**
 * Create an ellipse element, optionally with centered label text.
 */
export function ellipse(
	x: number, y: number, w: number, h: number,
	opts: ShapeOptions = {},
): ExcalidrawElement[] {
	const colors = applySemanticColors(opts, opts.semantic);
	const id = opts.id || newId();
	const boundElements: BoundElement[] = [];
	const elements: ExcalidrawElement[] = [];

	if (opts.label) {
		const textId = newId();
		boundElements.push({ id: textId, type: "text" });
		const fontSize = opts.labelFontSize ?? DEFAULT_FONT_SIZE;
		const dims = measureTextApprox(opts.label, fontSize);
		elements.push({
			...createBaseElement("text", x + (w - dims.width) / 2, y + (h - dims.height) / 2, dims.width, dims.height, {
				id: textId,
				strokeColor: textColorForBackground(
					colors.backgroundColor ?? DEFAULT_ELEMENT_STYLE.backgroundColor,
					colors.strokeColor,
				),
			}),
			type: "text",
			text: opts.label, fontSize, fontFamily: DEFAULT_FONT_FAMILY,
			textAlign: "center" as TextAlign, verticalAlign: "middle" as VerticalAlign,
			containerId: id, originalText: opts.label, autoResize: true, lineHeight: DEFAULT_LINE_HEIGHT,
		} as TextElement);
	}

	elements.unshift({
		...createBaseElement("ellipse", x, y, w, h, {
			id,
			roundness: null,
			boundElements: boundElements.length > 0 ? boundElements : null,
			fillStyle: opts.fillStyle,
			strokeWidth: opts.strokeWidth,
			strokeStyle: opts.strokeStyle,
			roughness: opts.roughness,
			groupIds: opts.groupIds,
			...colors,
		}),
		type: "ellipse",
	} as EllipseElement);

	return elements;
}

/**
 * Create a small marker dot (10-12px filled circle).
 * Returns `[EllipseElement]` for consistency with other shape factories.
 */
export function dot(
	x: number, y: number,
	opts: { id?: string; semantic?: SemanticPurpose; size?: number } = {},
): ExcalidrawElement[] {
	const size = opts.size ?? 12;
	const colors = opts.semantic ? SEMANTIC_COLORS[opts.semantic] : SEMANTIC_COLORS.primary;
	return [{
		...createBaseElement("ellipse", x - size / 2, y - size / 2, size, size, {
			id: opts.id,
			strokeColor: colors.stroke,
			backgroundColor: colors.fill,
			strokeWidth: 1,
		}),
		type: "ellipse",
	} as EllipseElement];
}

// ===================================================================
// PUBLIC API — Text
// ===================================================================

export interface TextOptions {
	id?: string;
	level?: TextLevel;
	fontSize?: number;
	fontFamily?: number;
	textAlign?: TextAlign;
	verticalAlign?: VerticalAlign;
	strokeColor?: string;
}

/**
 * Create a free-floating text element (not bound to any container).
 */
export function text(
	x: number, y: number,
	content: string,
	opts: TextOptions = {},
): TextElement {
	const fontSize = opts.fontSize ?? (
		opts.level === "title" ? 28 :
		opts.level === "subtitle" ? 20 : DEFAULT_FONT_SIZE
	);
	const dims = measureTextApprox(content, fontSize);
	const color = opts.strokeColor ?? (opts.level ? TEXT_COLORS[opts.level] : TEXT_COLORS.body);

	return {
		...createBaseElement("text", x, y, dims.width, dims.height, {
			id: opts.id,
			strokeColor: color,
		}),
		type: "text",
		text: content,
		fontSize,
		fontFamily: opts.fontFamily ?? DEFAULT_FONT_FAMILY,
		textAlign: opts.textAlign ?? "left",
		verticalAlign: opts.verticalAlign ?? "top",
		containerId: null,
		originalText: content,
		autoResize: true,
		lineHeight: DEFAULT_LINE_HEIGHT,
	} as TextElement;
}

// ===================================================================
// PUBLIC API — Connectors
// ===================================================================

export interface ArrowOptions {
	id?: string;
	semantic?: SemanticPurpose;
	strokeColor?: string;
	strokeWidth?: number;
	strokeStyle?: StrokeStyle;
	startArrowhead?: Arrowhead;
	endArrowhead?: Arrowhead;
	label?: string;
	/** Intermediate waypoints relative to start (x,y). Start [0,0] and end are auto-added. */
	waypoints?: Array<[number, number]>;
}

/**
 * Create an arrow connecting two points.
 * If `fromId`/`toId` are provided, creates bindings to those elements.
 */
export function arrow(
	x1: number, y1: number, x2: number, y2: number,
	opts: ArrowOptions & { fromId?: string; toId?: string } = {},
): ExcalidrawElement[] {
	const id = opts.id || newId();
	const dx = x2 - x1;
	const dy = y2 - y1;
	const points: Array<[number, number]> = opts.waypoints
		? [[0, 0], ...opts.waypoints, [dx, dy]]
		: [[0, 0], [dx, dy]];

	const colors = opts.semantic ? SEMANTIC_COLORS[opts.semantic] : undefined;
	const strokeColor = opts.strokeColor ?? colors?.stroke ?? DEFAULT_ELEMENT_STYLE.strokeColor;

	const boundElements: BoundElement[] = [];
	const elements: ExcalidrawElement[] = [];

	// Arrow label
	if (opts.label) {
		const textId = newId();
		boundElements.push({ id: textId, type: "text" });
		const fontSize = DEFAULT_FONT_SIZE;
		const dims = measureTextApprox(opts.label, fontSize);
		const midX = x1 + dx / 2 - dims.width / 2;
		const midY = y1 + dy / 2 - dims.height / 2;
		elements.push({
			...createBaseElement("text", midX, midY, dims.width, dims.height, {
				id: textId,
				strokeColor: TEXT_COLORS.body,
			}),
			type: "text",
			text: opts.label, fontSize, fontFamily: DEFAULT_FONT_FAMILY,
			textAlign: "center" as TextAlign, verticalAlign: "middle" as VerticalAlign,
			containerId: id, originalText: opts.label, autoResize: true, lineHeight: DEFAULT_LINE_HEIGHT,
		} as TextElement);
	}

	let minX = 0, maxX = 0, minY = 0, maxY = 0;
	for (const [px, py] of points) {
		minX = Math.min(minX, px);
		maxX = Math.max(maxX, px);
		minY = Math.min(minY, py);
		maxY = Math.max(maxY, py);
	}

	const startBinding: ArrowBinding | null = opts.fromId
		? { elementId: opts.fromId, mode: "orbit", fixedPoint: [0.5, 0.5] }
		: null;
	const endBinding: ArrowBinding | null = opts.toId
		? { elementId: opts.toId, mode: "orbit", fixedPoint: [0.5, 0.5] }
		: null;

	const arrowEl: ArrowElement = {
		...createBaseElement("arrow", x1, y1, maxX - minX, maxY - minY, {
			id,
			strokeColor,
			strokeWidth: opts.strokeWidth,
			strokeStyle: opts.strokeStyle,
			roundness: { type: 2 },
			boundElements: boundElements.length > 0 ? boundElements : null,
		}),
		type: "arrow",
		points,
		lastCommittedPoint: null,
		startBinding,
		endBinding,
		startArrowhead: opts.startArrowhead ?? null,
		endArrowhead: opts.endArrowhead ?? "arrow",
		elbowed: false,
	} as ArrowElement;

	elements.unshift(arrowEl);
	return elements;
}

/**
 * Create a structural line (not an arrow — no arrowheads).
 */
export function line(
	points: Array<[number, number]>,
	opts: {
		id?: string;
		strokeColor?: string;
		strokeWidth?: number;
		strokeStyle?: StrokeStyle;
	} = {},
): LineElement {
	if (points.length < 2) throw new Error("Line requires at least 2 points");
	const [x, y] = points[0];
	const relativePoints = points.map(([px, py]) => [px - x, py - y] as [number, number]);

	let minX = 0, maxX = 0, minY = 0, maxY = 0;
	for (const [px, py] of relativePoints) {
		minX = Math.min(minX, px);
		maxX = Math.max(maxX, px);
		minY = Math.min(minY, py);
		maxY = Math.max(maxY, py);
	}

	return {
		...createBaseElement("line", x, y, maxX - minX, maxY - minY, {
			id: opts.id,
			strokeColor: opts.strokeColor ?? "#64748b",
			strokeWidth: opts.strokeWidth ?? 2,
			strokeStyle: opts.strokeStyle,
		}),
		type: "line",
		points: relativePoints,
		lastCommittedPoint: null,
		startBinding: null,
		endBinding: null,
		startArrowhead: null,
		endArrowhead: null,
	} as LineElement;
}

// ===================================================================
// PUBLIC API — Binding Wiring
// ===================================================================

/**
 * Wire an arrow to source/target elements, updating boundElements on both ends.
 * Mutates the elements array in-place.
 */
export function bindArrow(
	elements: ExcalidrawElement[],
	arrowId: string,
	startId: string,
	endId: string,
): void {
	const arrowEl = elements.find((e) => e.id === arrowId) as ArrowElement | undefined;
	const startEl = elements.find((e) => e.id === startId);
	const endEl = elements.find((e) => e.id === endId);

	if (!arrowEl || arrowEl.type !== "arrow") throw new Error(`Arrow '${arrowId}' not found`);
	if (!startEl) throw new Error(`Start element '${startId}' not found`);
	if (!endEl) throw new Error(`End element '${endId}' not found`);

	// Set bindings on arrow
	arrowEl.startBinding = { elementId: startId, mode: "orbit", fixedPoint: [0.5, 0.5] };
	arrowEl.endBinding = { elementId: endId, mode: "orbit", fixedPoint: [0.5, 0.5] };

	// Add arrow to both elements' boundElements
	const binding: BoundElement = { id: arrowId, type: "arrow" };

	if (!startEl.boundElements) (startEl as any).boundElements = [];
	if (!(startEl.boundElements as BoundElement[]).some((b) => b.id === arrowId)) {
		(startEl.boundElements as BoundElement[]).push(binding);
	}

	if (!endEl.boundElements) (endEl as any).boundElements = [];
	if (!(endEl.boundElements as BoundElement[]).some((b) => b.id === arrowId)) {
		(endEl.boundElements as BoundElement[]).push(binding);
	}
}

// ===================================================================
// PUBLIC API — Document Builder
// ===================================================================

/**
 * Wrap elements in a complete .excalidraw document.
 * Resets the index counter so the next document starts fresh.
 */
export function createDocument(
	elements: ExcalidrawElement[],
	opts: { background?: string } = {},
): ExcalidrawFile {
	resetIndexCounter();
	return {
		type: "excalidraw",
		version: 2,
		source: "https://excalidraw.com",
		elements,
		appState: {
			...DEFAULT_APP_STATE,
			viewBackgroundColor: opts.background ?? DEFAULT_APP_STATE.viewBackgroundColor,
		},
		files: {},
	};
}

// ===================================================================
// PUBLIC API — Validation
// ===================================================================

/**
 * Validate an ExcalidrawFile, returning a list of errors (empty = valid).
 */
export function validateDocument(doc: ExcalidrawFile): string[] {
	const errors: string[] = [];

	if (doc.type !== "excalidraw") errors.push(`Expected type 'excalidraw', got '${doc.type}'`);
	if (doc.version !== 2) errors.push(`Expected version 2, got ${doc.version}`);
	if (!Array.isArray(doc.elements)) errors.push("'elements' must be an array");
	if (doc.elements.length === 0) errors.push("'elements' array is empty");

	const ids = new Set<string>();
	for (const el of doc.elements) {
		if (!el.id) errors.push(`Element missing 'id'`);
		if (ids.has(el.id)) errors.push(`Duplicate element id: '${el.id}'`);
		ids.add(el.id);
	}

	// Check binding references
	for (const el of doc.elements) {
		if (el.type === "arrow") {
			const a = el as ArrowElement;
			if (a.startBinding && !ids.has(a.startBinding.elementId)) {
				errors.push(`Arrow '${el.id}' startBinding references missing element '${a.startBinding.elementId}'`);
			}
			if (a.endBinding && !ids.has(a.endBinding.elementId)) {
				errors.push(`Arrow '${el.id}' endBinding references missing element '${a.endBinding.elementId}'`);
			}
		}
		if (el.type === "text") {
			const t = el as TextElement;
			if (t.containerId && !ids.has(t.containerId)) {
				errors.push(`Text '${el.id}' containerId references missing element '${t.containerId}'`);
			}
		}
		if (el.boundElements) {
			for (const b of el.boundElements) {
				if (!ids.has(b.id)) {
					errors.push(`Element '${el.id}' boundElements references missing element '${b.id}'`);
				}
			}
		}
	}

	return errors;
}

// ===================================================================
// PUBLIC API — Layout Helpers
// ===================================================================

export type Point = [number, number];

/**
 * Generate points for a fan-out pattern (one center → many targets).
 * Returns target positions arranged in an arc.
 */
export function fanOut(
	center: Point, count: number, radius: number,
	opts: { arc?: number; startAngle?: number } = {},
): Point[] {
	const arc = opts.arc ?? Math.PI; // 180° default
	const startAngle = opts.startAngle ?? -arc / 2;
	const step = count > 1 ? arc / (count - 1) : 0;

	return Array.from({ length: count }, (_, i) => {
		const angle = startAngle + step * i;
		return [
			center[0] + Math.cos(angle) * radius,
			center[1] + Math.sin(angle) * radius,
		] as Point;
	});
}

/**
 * Generate evenly-spaced points along a line (for timelines).
 */
export function timeline(
	start: Point, count: number, spacing: number,
	direction: "horizontal" | "vertical" = "vertical",
): Point[] {
	return Array.from({ length: count }, (_, i) =>
		direction === "vertical"
			? [start[0], start[1] + i * spacing] as Point
			: [start[0] + i * spacing, start[1]] as Point,
	);
}

/**
 * Generate grid positions (row-major order).
 */
export function grid(
	origin: Point, cols: number, rows: number,
	cellW: number, cellH: number,
): Point[] {
	const points: Point[] = [];
	for (let r = 0; r < rows; r++) {
		for (let c = 0; c < cols; c++) {
			points.push([origin[0] + c * cellW, origin[1] + r * cellH]);
		}
	}
	return points;
}
