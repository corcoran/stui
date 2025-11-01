# Ratatui UI Implementation Notes

## Table of Contents
1. [Constraint System](#constraint-system)
2. [Dynamic Height Patterns](#dynamic-height-patterns)
3. [Text Wrapping and Height Calculation](#text-wrapping-and-height-calculation)
4. [Common Pitfalls](#common-pitfalls)
5. [Best Practices](#best-practices)

---

## Constraint System

### Constraint Types & Priority Order

Ratatui uses a **Cassowary constraint solver** with the following priority hierarchy:

**Priority 1 (Highest)**: `Constraint::Min(u16)` and `Constraint::Max(u16)`
- `Min`: Element size is set to at least the specified amount
- `Max`: Element size is set to at most the specified amount

**Priority 2**: `Constraint::Length(u16)`
- Element size is set to the specified amount (fixed size)

**Priority 3**: `Constraint::Percentage(u16)` and `Constraint::Ratio(u32, u32)`
- Proportional sizing relative to total available space

**Priority 4 (Lowest)**: `Constraint::Fill(u16)`
- Fills excess space proportionally, lower priority than spacers

### Constraint Interaction Behavior

#### Multiple `Min` Constraints
When multiple `Min` constraints compete in the same layout, the Cassowary solver tries to satisfy both with equal priority. This can lead to unexpected space allocation.

**Example Problem**:
```rust
Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Length(3),  // System bar
        Constraint::Min(3),     // Content area
        Constraint::Min(3),     // Legend (PROBLEM!)
        Constraint::Length(3),  // Status bar
    ])
```

**Issue**: Both content area and legend have `Min(3)`, so the solver may allocate excessive space to the legend to satisfy both constraints equally.

**Solution**: Use calculated `Length` for widgets that should fit content exactly.

#### Constraint Resolution Strategy

1. **Hard constraints first**: `Length` constraints are satisfied first (rigid)
2. **Min/Max bounds**: Applied to ensure minimums/maximums
3. **Proportional distribution**: `Percentage`, `Ratio`, `Fill` distribute remaining space

---

## Dynamic Height Patterns

### The `Paragraph::line_count()` Method

**Official API for dynamic height calculation**:

```rust
pub fn line_count(&self, width: u16) -> usize
```

**Purpose**: Calculates the exact number of lines needed to fully render a paragraph given a specific width, accounting for:
- Text wrapping with `Wrap { trim: false }`
- Block borders (automatically considers border space)
- Multi-line content with proper word wrapping

**Feature Flag Required**:
```toml
[dependencies]
ratatui = { version = "0.29", features = ["unstable-rendered-line-info"] }
```

**Stability Status**:
- Marked as "unstable" but maintainers confirm: "no current plans to change this in the current version"
- Safe to use despite unstable designation
- Tracking issue: ratatui/ratatui#293

**Example Usage**:
```rust
let paragraph = Paragraph::new("Hello World")
    .block(Block::default().borders(Borders::ALL))
    .wrap(Wrap { trim: false });

// Wide terminal: fits on one line
assert_eq!(paragraph.line_count(20), 1);

// Narrow terminal: wraps to two lines
assert_eq!(paragraph.line_count(10), 2);
```

### Two-Pass Layout Pattern

**Pattern**: Calculate required height before creating layout, then use `Length(calculated_height)`

**Step 1: Build Widget and Calculate Height**
```rust
fn calculate_widget_height(terminal_width: u16, content: &str) -> u16 {
    let paragraph = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: false });

    // Subtract borders from width
    let available_width = terminal_width.saturating_sub(2);
    let line_count = paragraph.line_count(available_width);

    // Add borders to height, enforce minimum
    (line_count as u16).saturating_add(2).max(3)
}
```

**Step 2: Use Calculated Height in Layout**
```rust
let widget_height = calculate_widget_height(terminal_width, content);

let chunks = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Min(5),              // Other content
        Constraint::Length(widget_height), // Dynamic widget
    ])
    .split(area);
```

### Alternative: Manual Line Counting

If `line_count()` is unavailable, you can calculate manually:

```rust
fn manual_line_count(text: &str, width: usize) -> usize {
    text.lines()
        .map(|line| {
            if line.is_empty() {
                1
            } else {
                ((line.len() + width - 1) / width).max(1)
            }
        })
        .sum()
}
```

**Limitations**:
- Doesn't handle complex Unicode width calculations
- Doesn't account for word boundaries (may break mid-word)
- Use `unicode-width` crate for accurate character width

---

## Text Wrapping and Height Calculation

### Border Accounting

**Critical**: Always account for borders when calculating dimensions.

**Width Calculation**:
```rust
// ❌ WRONG
let line_count = paragraph.line_count(terminal_width);

// ✅ CORRECT
let available_width = terminal_width.saturating_sub(2); // Left + right borders
let line_count = paragraph.line_count(available_width);
```

**Height Calculation**:
```rust
// ❌ WRONG
let total_height = line_count as u16;

// ✅ CORRECT
let total_height = (line_count as u16).saturating_add(2); // Top + bottom borders
```

### Minimum Height Enforcement

**Always enforce minimum dimensions** to prevent degenerate cases:

```rust
// ✅ Enforce minimum
let widget_height = calculated_height.max(3);
let widget_width = calculated_width.max(10);
```

**Why**: On extremely narrow/short terminals, calculations might return 0 or 1, causing layout issues.

### Wrapping Configuration

**Ratatui Wrap Options**:
```rust
pub struct Wrap {
    pub trim: bool,
}
```

- `trim: true` - Trim leading/trailing whitespace from wrapped lines
- `trim: false` - Preserve whitespace (better for pre-formatted text)

**Example**:
```rust
let paragraph = Paragraph::new(text)
    .wrap(Wrap { trim: false });  // Preserve formatting
```

---

## Common Pitfalls

### 1. Competing Min Constraints

**Problem**: Multiple `Min` constraints allocate space unpredictably.

**Bad**:
```rust
.constraints([
    Constraint::Min(3),  // Widget A
    Constraint::Min(5),  // Widget B
])
```

**Good**:
```rust
.constraints([
    Constraint::Min(3),       // Widget A (flexible)
    Constraint::Length(5),    // Widget B (fixed)
])
```

### 2. Forgetting Border Space

**Problem**: Miscalculating dimensions by forgetting borders.

**Fix**: Always subtract/add 2 for borders:
- Width: `available = total - 2`
- Height: `total = content + 2`

### 3. Static vs Owned Data

**Problem**: `Paragraph` needs ownership of text data.

**Fix**: Use owned types (`String`, `Vec<Span>`) instead of references:
```rust
// ✅ Owned data
let line = Line::from(vec![Span::raw(text.to_string())]);

// ❌ Borrowed data (may cause lifetime issues)
let line = Line::from(vec![Span::raw(text)]);
```

### 4. Not Caching Calculations

**Problem**: Recalculating layout on every frame is expensive.

**Fix**: Cache calculated heights and only recompute when state changes:
```rust
struct AppState {
    cached_legend_height: u16,
    last_terminal_width: u16,
    last_vim_mode: bool,
}

impl AppState {
    fn update_legend_height(&mut self, terminal_width: u16, vim_mode: bool) {
        if self.last_terminal_width != terminal_width || self.last_vim_mode != vim_mode {
            self.cached_legend_height = calculate_legend_height(terminal_width, vim_mode);
            self.last_terminal_width = terminal_width;
            self.last_vim_mode = vim_mode;
        }
    }
}
```

### 5. Clipping with Fixed Constraints

**Problem**: Using `Length` without calculating actual needs clips content.

**Before** (clips on narrow terminals):
```rust
Constraint::Length(3)  // Always 3 lines, wraps and clips
```

**After** (grows as needed):
```rust
let height = calculate_height(width);
Constraint::Length(height)  // Exact fit
```

---

## Best Practices

### 1. Use Official APIs When Available

✅ **Prefer**: `Paragraph::line_count()` for height calculation
❌ **Avoid**: Manual line counting unless necessary

**Why**: Official APIs handle edge cases you might miss.

### 2. Calculate Once, Use Everywhere

Extract widget building into reusable functions:

```rust
// Reusable builder
pub fn build_legend_paragraph(params: &LegendParams) -> Paragraph<'static> {
    // ... build paragraph
}

// Height calculation
pub fn calculate_legend_height(width: u16, params: &LegendParams) -> u16 {
    let paragraph = build_legend_paragraph(params);
    let line_count = paragraph.line_count(width.saturating_sub(2));
    (line_count as u16).saturating_add(2).max(3)
}

// Rendering
pub fn render_legend(f: &mut Frame, area: Rect, params: &LegendParams) {
    let legend = build_legend_paragraph(params);
    f.render_widget(legend, area);
}
```

### 3. Test on Various Terminal Sizes

**Test Matrix**:
- **Wide**: 120+ columns (should be minimal height)
- **Medium**: 60-120 columns (moderate wrapping)
- **Narrow**: 40-60 columns (significant wrapping)
- **Extreme**: < 40 columns (maximum wrapping, may clip)

**Unit Test Example**:
```rust
#[test]
fn test_legend_height_wide_terminal() {
    let height = calculate_legend_height(120, false, 1, false, true);
    assert_eq!(height, 3); // Minimum height
}

#[test]
fn test_legend_height_narrow_terminal() {
    let height = calculate_legend_height(40, false, 1, false, true);
    assert!(height > 3); // Should wrap
}
```

### 4. Document Layout Constraints

**Add comments explaining constraint choices**:

```rust
Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Length(3),            // System bar (fixed height)
        Constraint::Min(3),               // Content area (flexible, expands)
        Constraint::Length(legend_height), // Legend (fits content exactly)
        Constraint::Length(3),            // Status bar (fixed height)
    ])
```

### 5. Handle Degenerate Cases

**Always handle edge cases gracefully**:

```rust
fn calculate_height(width: u16) -> u16 {
    // Handle zero/tiny widths
    if width < 10 {
        return 3; // Minimum viable height
    }

    let available_width = width.saturating_sub(2);
    let line_count = paragraph.line_count(available_width);

    // Enforce reasonable bounds
    (line_count as u16)
        .saturating_add(2)  // Add borders
        .max(3)             // Minimum
        .min(20)            // Maximum (prevent excessive height)
}
```

---

## References

### Official Documentation
- Ratatui Constraints: https://docs.rs/ratatui/latest/ratatui/layout/enum.Constraint.html
- Paragraph::line_count: https://docs.rs/ratatui/latest/ratatui/widgets/struct.Paragraph.html#method.line_count
- Layout System: https://ratatui.rs/concepts/layout/
- Dynamic Layouts Recipe: https://ratatui.rs/recipes/layout/dynamic/

### GitHub Issues
- Tracking issue for line_count: ratatui/ratatui#293
- Border accounting bug: ratatui/ratatui#1233
- Dynamic height request: ratatui/ratatui#1365

### Community Resources
- Forum discussion: https://forum.ratatui.rs/t/height-on-width-constrained-paragraph/156
- Examples repository: https://github.com/ratatui-org/ratatui/tree/main/examples

---

## Implementation Checklist

When implementing dynamic height widgets:

- [ ] Enable `unstable-rendered-line-info` feature flag in Cargo.toml
- [ ] Extract widget building into reusable function
- [ ] Create height calculation function using `line_count()`
- [ ] Account for borders (subtract 2 from width, add 2 to height)
- [ ] Enforce minimum dimensions (`.max(3)`)
- [ ] Update layout to use `Constraint::Length(calculated_height)`
- [ ] Pass all required parameters to layout calculation
- [ ] Add unit tests for various terminal widths
- [ ] Test manually on narrow terminals
- [ ] Consider caching calculated heights for performance
- [ ] Document constraint choices in comments
- [ ] Handle edge cases (zero width, extreme narrow, etc.)
