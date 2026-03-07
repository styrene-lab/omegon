/**
 * Excalidraw element factory — programmatic .excalidraw file generation.
 *
 * Usage in LLM tool calls:
 *   import { rect, diamond, ellipse, dot, text, arrow, line,
 *            bindArrow, createDocument, validateDocument,
 *            fanOut, timeline, grid,
 *            SEMANTIC_COLORS, TEXT_COLORS } from "./excalidraw/index.ts";
 *
 * See UPSTREAM.md for vendoring provenance and sync instructions.
 */

export {
	// Shape factories
	rect,
	diamond,
	ellipse,
	dot,
	// Text
	text,
	// Connectors
	arrow,
	line,
	// Binding
	bindArrow,
	// Document
	createDocument,
	validateDocument,
	resetIndexCounter,
	// Layout helpers
	fanOut,
	timeline,
	grid,
} from "./elements.ts";

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
	Point,
	ShapeOptions,
	TextOptions,
	ArrowOptions,
} from "./elements.ts";

export {
	SEMANTIC_COLORS,
	TEXT_COLORS,
	FONT_FAMILIES,
	DEFAULT_ELEMENT_STYLE,
	DEFAULT_APP_STATE,
} from "./types.ts";

export type {
	ColorPair,
	AppState,
	ArrowBinding,
	BoundElement,
} from "./types.ts";
