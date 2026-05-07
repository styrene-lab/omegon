+++
id = "daecd2b2-8805-483b-9976-91829022e343"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Scry Image Generation Extension

The scry extension gives you local image generation capabilities using diffusion models (FLUX, Stable Diffusion, SDXL). Images are generated on the user's machine — no external API calls.

## Available tools

- **generate** — Text-to-image. Requires a `prompt` and `model`. Supports LoRA stacking via `loras`.
- **refine** — Image-to-image. Takes an existing image and transforms it with a text prompt and strength parameter.
- **upscale** — Super-resolution. Upscale an image by 2x or 4x.
- **list_models** — Discover available local models. Call this first to find valid model names.
- **scan_models** — Re-scan model directories if the user has added new models.
- **compose_workflow** — Generate a raw ComfyUI API-format workflow JSON for advanced users.
- **search_models** — Search HuggingFace Hub or CivitAI for downloadable models.
- **download_model** — Download a model from HuggingFace Hub.

## Guidelines

- Always call `list_models` before `generate` or `refine` to discover which models are available locally. Do not guess model names.
- When the user asks to generate an image, craft a detailed prompt. Good prompts are specific about subject, style, lighting, composition, and medium.
- Use `negative_prompt` to steer away from common artifacts (e.g. "blurry, low quality, deformed").
- Default to reasonable dimensions (1024x1024 for SDXL/FLUX, 512x512 for SD1.5) unless the user specifies otherwise.
- For iterative workflows: generate first, then refine with lower strength (0.3–0.5) for subtle changes or higher strength (0.6–0.8) for major reworks.
- LoRAs are applied in order. Use `list_models` with `kind: "lora"` to discover available LoRAs.
- Generated images are saved to the output directory. Report the output path to the user.
