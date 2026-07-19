# Codasaurus Dashboard — Design System

## 1. Brand Essence

Developer tool for AI-generated code review. Clean, technical, confident. Inspired by Linear, Vercel, and Stripe — precise spacing, restrained color, functional beauty. The interface should feel fast before it loads.

## 2. Color Palette

### Light Mode
- `--bg-primary`: `#ffffff`
- `--bg-secondary`: `#f6f6f6`
- `--bg-tertiary`: `#eeeeee`
- `--surface`: `#fafafa`
- `--text-primary`: `#0a0a0a`
- `--text-secondary`: `#5a5a5a`
- `--text-muted`: `#9a9a9a`
- `--border`: `#e4e4e4`
- `--border-light`: `#f0f0f0`
- `--accent`: `#0a0a0a`
- `--accent-hover`: `#2a2a2a`
- `--error`: `#dc2626` (desaturated red for less screaming)
- `--success`: `#16a34a`
- `--warning`: `#d97706`
- `--info`: `#2563eb`

### Dark Mode
- `--bg-primary`: `#0a0a0a`
- `--bg-secondary`: `#121212`
- `--bg-tertiary`: `#1a1a1a`
- `--surface`: `#141414`
- `--text-primary`: `#f0f0f0`
- `--text-secondary`: `#8a8a8a`
- `--text-muted`: `#555555`
- `--border`: `#1e1e1e`
- `--border-light`: `#181818`
- `--accent`: `#f0f0f0`
- `--accent-hover`: `#d0d0d0`
- `--error`: `#ef4444`
- `--success`: `#22c55e`
- `--warning`: `#f59e0b`
- `--info`: `#3b82f6`

### Shadows (light)
- `--shadow-sm`: `0 1px 2px rgba(0,0,0,0.04)`
- `--shadow-md`: `0 2px 8px rgba(0,0,0,0.06), 0 1px 3px rgba(0,0,0,0.04)`
- `--shadow-lg`: `0 4px 24px rgba(0,0,0,0.08), 0 2px 6px rgba(0,0,0,0.04)`

### Shadows (dark)
- `--shadow-sm`: `0 1px 2px rgba(0,0,0,0.3)`
- `--shadow-md`: `0 2px 8px rgba(0,0,0,0.4)`
- `--shadow-lg`: `0 4px 24px rgba(0,0,0,0.5)`

## 3. Typography

### Font Family
- Body: `"Inter", -apple-system, BlinkMacSystemFont, sans-serif`
- Mono: `"JetBrains Mono", "SF Mono", "Fira Code", monospace`

### Scale
- `--text-xs`: `11px` (labels, captions)
- `--text-sm`: `13px` (secondary text, meta)
- `--text-base`: `14px` (body)
- `--text-lg`: `16px` (card titles)
- `--text-xl`: `18px` (section headings)
- `--text-2xl`: `24px` (page titles)
- `--text-3xl`: `32px` (hero headings)
- `--text-4xl`: `48px` (display)

### Weights
- Regular: 400
- Medium: 500
- Semibold: 600
- Bold: 700

## 4. Spacing Scale

`--space-1`: `4px`
`--space-2`: `8px`
`--space-3`: `12px`
`--space-4`: `16px`
`--space-5`: `20px`
`--space-6`: `24px`
`--space-8`: `32px`
`--space-10`: `40px`
`--space-12`: `48px`
`--space-16`: `64px`

## 5. Components

### Buttons
- Border radius: `8px`
- Padding: `8px 20px` (default), `10px 24px` (lg)
- Transition: `all 0.15s cubic-bezier(0.4, 0, 0.2, 1)`
- Hover: subtle lift (`translateY(-1px)`) + darker background
- Active: `scale(0.98)`
- Variants: primary (filled accent), secondary (outlined), ghost (borderless), danger (red)

### Cards
- Border radius: `10px`
- Background: `var(--surface)` 
- Border: `1px solid var(--border)`
- Padding: `20px`
- Hover: subtle shadow elevation + `translateY(-1px)`
- Transition: `all 0.2s cubic-bezier(0.4, 0, 0.2, 1)`

### Inputs
- Border radius: `8px`
- Padding: `10px 14px`
- Border: `1px solid var(--border)`
- Focus: ring `0 0 0 2px var(--accent)` at 15% opacity
- Transition: `all 0.15s ease`

### Sidebar
- Width: `240px`
- Border-right
- Nav items: `10px 20px`, border-radius `6px`, icon + label layout
- Active: subtle background + medium weight

### Wizard
- Max width: `520px`
- Step indicator: numbered circles with connecting line
- Card: centered, bordered

### Toggle
- Width: `38px`, height: `22px`
- Border radius: `11px` (pill)
- Transition: background 0.2s, knob position 0.2s

### Badges / Status
- Border radius: `6px`
- Padding: `2px 8px`
- Font: `11px`, semibold

## 6. Animation & Motion

- **Page transitions**: fade + subtle slide-up (`translateY(8px) → 0`, opacity `0 → 1`), 250ms
- **Hover interactions**: `translateY(-1px)` lift on cards, buttons; `opacity` changes on icons
- **Loading**: skeleton shimmer with `@keyframes shimmer` (pulsing gradient)
- **Focus ring**: `box-shadow` 0 0 0 2px accent at low opacity
- **Reduce motion**: respect `prefers-reduced-motion` — disable non-essential animations
- **Transitions**: `cubic-bezier(0.4, 0, 0.2, 1)` for all interactive elements

## 7. Dark Mode

Supported via `data-theme="dark"` attribute on `<html>`. All colors switch to dark variants. Icons and interactive states maintain same behavior. Toggle in header.
