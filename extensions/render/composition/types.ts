/**
 * Core types for the composition render pipeline.
 * Browser-free React → video/image renderer using Satori + resvg + gifenc.
 */

/**
 * Props injected into every frame component.
 * Mirrors the Remotion useCurrentFrame() / useVideoConfig() pattern
 * but passed directly as props instead of via hooks.
 */
export interface FrameProps {
  /** Current frame index (0-based) */
  frame: number;
  /** Frames per second */
  fps: number;
  /** Total number of frames in the composition */
  durationInFrames: number;
  /** Canvas width in pixels */
  width: number;
  /** Canvas height in pixels */
  height: number;
  /** User-defined props passed through from the render tool */
  props?: Record<string, unknown>;
}
