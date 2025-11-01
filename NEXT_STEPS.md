# Next Steps - Incremental Functional Migration

**Last Updated:** 2025-10-31
**Current Phase:** Step 3 - Extract Pure Business Logic (7/15 functions complete)

---

## 🎯 Current Status

### ✅ Completed

**Phase 1: Modularization** (27.7% code reduction)
- Extracted handlers, services, logic modules
- 48 tests passing, zero compilation errors

**Phase 2: Pure Model**
- Sub-model architecture (Syncthing, Navigation, UI, Performance)
- 100% state fields migrated to pure Model
- ListState → Option<usize> conversion
- Always-render approach (no dirty flags)
- Image preview regression fixed

**Phase 3: Pure Business Logic** (7/15 complete)
- ✅ `logic::folder::has_local_changes()` - 3 tests
- ✅ `logic::folder::can_delete_file()` - 4 tests
- ✅ `logic::folder::should_show_restore_button()` - 4 tests
- ✅ `logic::ui::cycle_display_mode()` - 3 tests
- ✅ `logic::ui::cycle_sort_mode()` - 2 tests
- ✅ `logic::ignore::validate_pattern()` - 6 tests
- ✅ `logic::navigation::next_selection()` + `prev_selection()` - 10 tests

### 📊 Test Coverage

- **80 tests passing** (63 logic + model, 17 state)
- **Zero compilation errors**
- **4 warnings** (unused imports/variables + unused function for future use - harmless)

---

## 🚀 Immediate Next Steps

### Step 1: Extract Validation Functions (Priority: HIGH)

These are low-hanging fruit - simple validation logic currently scattered across handlers.

#### Target 1: `can_delete_file()`
**Location:** `src/main.rs:1894-1920` (delete_file function)
**Current Logic:**
```rust
// Only works when focused on a breadcrumb level (not folder list)
if self.model.navigation.focus_level == 0 || self.model.navigation.breadcrumb_trail.is_empty() {
    return Ok(());
}
```

**Extract To:** `src/logic/folder.rs`
```rust
/// Check if file deletion is allowed given current navigation state
pub fn can_delete_file(focus_level: usize, breadcrumb_trail_empty: bool) -> bool {
    focus_level > 0 && !breadcrumb_trail_empty
}

#[test]
fn test_can_delete_file() {
    assert!(can_delete_file(1, false));   // In folder view - OK
    assert!(!can_delete_file(0, false));  // In folder list - NOT OK
    assert!(!can_delete_file(1, true));   // No breadcrumbs - NOT OK
}
```

**Usage:**
```rust
// Before (inline check)
if self.model.navigation.focus_level == 0 || self.model.navigation.breadcrumb_trail.is_empty() {
    return Ok(());
}

// After (pure function)
if !logic::folder::can_delete_file(
    self.model.navigation.focus_level,
    self.model.navigation.breadcrumb_trail.is_empty()
) {
    return Ok(());
}
```

**Estimated Time:** 15 minutes
**Tests:** 3-4 tests covering edge cases

---

#### Target 2: `should_show_restore_button()`
**Location:** `src/ui/hotkey.rs:*` (hotkey legend rendering)
**Current Logic:** Multiple places check `folder_status.receive_only_total_items > 0`

**Extract To:** `src/logic/folder.rs`
```rust
/// Check if the Restore button should be shown
///
/// Restore is only available for receive-only folders with local changes
pub fn should_show_restore_button(
    focus_level: usize,
    folder_status: Option<&FolderStatus>
) -> bool {
    focus_level == 0 && has_local_changes(folder_status)
}

#[test]
fn test_should_show_restore_button() {
    let status_with_changes = create_test_status(5);
    let status_no_changes = create_test_status(0);

    // Show restore: folder list + has changes
    assert!(should_show_restore_button(0, Some(&status_with_changes)));

    // Don't show: no changes
    assert!(!should_show_restore_button(0, Some(&status_no_changes)));

    // Don't show: in breadcrumb view
    assert!(!should_show_restore_button(1, Some(&status_with_changes)));

    // Don't show: no status
    assert!(!should_show_restore_button(0, None));
}
```

**Estimated Time:** 20 minutes
**Tests:** 4 tests covering combinations

---

#### Target 3: `validate_ignore_pattern()`
**Location:** Pattern input validation (currently missing!)
**Purpose:** Validate .stignore pattern syntax before sending to API

**Extract To:** `src/logic/ignore.rs` (add to existing ignore module)
```rust
/// Validate an ignore pattern for common syntax errors
///
/// Returns `Ok(())` if valid, `Err(message)` with helpful error
pub fn validate_pattern(pattern: &str) -> Result<(), String> {
    if pattern.trim().is_empty() {
        return Err("Pattern cannot be empty".to_string());
    }

    if pattern.contains('\n') {
        return Err("Pattern cannot contain newlines".to_string());
    }

    // Check for unclosed brackets
    let open_brackets = pattern.matches('[').count();
    let close_brackets = pattern.matches(']').count();
    if open_brackets != close_brackets {
        return Err("Unclosed bracket in pattern".to_string());
    }

    Ok(())
}

#[test]
fn test_validate_pattern() {
    assert!(validate_pattern("*.jpg").is_ok());
    assert!(validate_pattern("temp/[ab].txt").is_ok());
    assert!(validate_pattern("").is_err());
    assert!(validate_pattern("  ").is_err());
    assert!(validate_pattern("test[.txt").is_err());
}
```

**Estimated Time:** 25 minutes
**Tests:** 5-6 tests covering valid/invalid patterns

---

### Step 2: Extract UI State Transitions (Priority: MEDIUM)

These are simple state cycling functions - perfect for pure logic.

#### Target 4: `cycle_sort_mode()`
**Location:** `src/main.rs:123-131` (SortMode impl)
**Current:** Has a `next()` method on the enum

**Improve:** Move to dedicated module with validation
```rust
// src/logic/ui.rs (NEW FILE)
use crate::SortMode;

/// Cycle to the next sort mode in sequence
pub fn cycle_sort_mode(current: SortMode, focus_level: usize) -> Option<SortMode> {
    // No sorting for folder list
    if focus_level == 0 {
        return None;
    }

    Some(match current {
        SortMode::VisualIndicator => SortMode::Alphabetical,
        SortMode::Alphabetical => SortMode::LastModified,
        SortMode::LastModified => SortMode::FileSize,
        SortMode::FileSize => SortMode::VisualIndicator,
    })
}

#[test]
fn test_cycle_sort_mode() {
    use SortMode::*;

    // Normal cycling in breadcrumb view
    assert_eq!(cycle_sort_mode(VisualIndicator, 1), Some(Alphabetical));
    assert_eq!(cycle_sort_mode(Alphabetical, 1), Some(LastModified));
    assert_eq!(cycle_sort_mode(LastModified, 1), Some(FileSize));
    assert_eq!(cycle_sort_mode(FileSize, 1), Some(VisualIndicator));

    // No cycling in folder list
    assert_eq!(cycle_sort_mode(Alphabetical, 0), None);
}
```

**Usage:**
```rust
// In handlers/keyboard.rs
KeyCode::Char('s') => {
    if let Some(new_mode) = logic::ui::cycle_sort_mode(
        app.model.ui.sort_mode,
        app.model.navigation.focus_level
    ) {
        app.model.ui.sort_mode = new_mode;
        app.model.ui.sort_reverse = false;
        app.sort_all_levels();
    }
}
```

**Estimated Time:** 20 minutes
**Tests:** 5 tests (4 cycle states + folder list case)

---

#### Target 5: `cycle_display_mode()`
**Location:** `src/main.rs:105-113` (DisplayMode impl)

**Extract To:** `src/logic/ui.rs`
```rust
use crate::DisplayMode;

/// Cycle to the next display mode: Off → TimestampOnly → TimestampAndSize → Off
pub fn cycle_display_mode(current: DisplayMode) -> DisplayMode {
    match current {
        DisplayMode::Off => DisplayMode::TimestampOnly,
        DisplayMode::TimestampOnly => DisplayMode::TimestampAndSize,
        DisplayMode::TimestampAndSize => DisplayMode::Off,
    }
}

#[test]
fn test_cycle_display_mode() {
    use DisplayMode::*;
    assert_eq!(cycle_display_mode(Off), TimestampOnly);
    assert_eq!(cycle_display_mode(TimestampOnly), TimestampAndSize);
    assert_eq!(cycle_display_mode(TimestampAndSize), Off);
}
```

**Estimated Time:** 10 minutes
**Tests:** 3 tests (one per transition)

---

### Step 3: Extract Navigation Calculations (Priority: LOW)

These are more complex but still pure calculations.

#### Target 6: `calculate_next_selection()`
**Location:** `src/main.rs:1651-1715` (next_item/previous_item functions)

**Current:** Complex inline logic with wrapping behavior

**Extract To:** `src/logic/navigation.rs` (NEW FILE)
```rust
/// Calculate the next selection index with wrapping
pub fn next_selection(current: Option<usize>, list_len: usize) -> Option<usize> {
    if list_len == 0 {
        return None;
    }

    Some(match current {
        Some(i) if i >= list_len - 1 => 0,  // Wrap to start
        Some(i) => i + 1,
        None => 0,
    })
}

/// Calculate the previous selection index with wrapping
pub fn prev_selection(current: Option<usize>, list_len: usize) -> Option<usize> {
    if list_len == 0 {
        return None;
    }

    Some(match current {
        Some(0) | None => list_len - 1,  // Wrap to end
        Some(i) => i - 1,
    })
}

#[test]
fn test_next_selection() {
    // Empty list
    assert_eq!(next_selection(None, 0), None);
    assert_eq!(next_selection(Some(0), 0), None);

    // Normal progression
    assert_eq!(next_selection(None, 3), Some(0));
    assert_eq!(next_selection(Some(0), 3), Some(1));
    assert_eq!(next_selection(Some(1), 3), Some(2));

    // Wrapping
    assert_eq!(next_selection(Some(2), 3), Some(0));
}

#[test]
fn test_prev_selection() {
    // Empty list
    assert_eq!(prev_selection(None, 0), None);

    // Normal progression
    assert_eq!(prev_selection(Some(2), 3), Some(1));
    assert_eq!(prev_selection(Some(1), 3), Some(0));

    // Wrapping
    assert_eq!(prev_selection(Some(0), 3), Some(2));
    assert_eq!(prev_selection(None, 3), Some(2));
}
```

**Estimated Time:** 30 minutes
**Tests:** 8-10 tests covering edge cases

---

## 📋 Complete Extraction Roadmap

### Phase A: Quick Wins (2-3 hours total) ✅ COMPLETE
- [x] `has_local_changes()` - 15 min ✅ DONE
- [x] `can_delete_file()` - 15 min ✅ DONE
- [x] `should_show_restore_button()` - 20 min ✅ DONE
- [x] `cycle_display_mode()` - 10 min ✅ DONE
- [x] `cycle_sort_mode()` - 20 min ✅ DONE

### Phase B: Moderate Complexity (2-3 hours total)
- [x] `validate_ignore_pattern()` - 25 min ✅ DONE
- [x] `calculate_next_selection()` / `prev_selection()` - 30 min ✅ DONE
- [ ] `toggle_sort_reverse()` - 15 min
- [ ] `next_vim_command_state()` - 20 min
- [ ] `should_dismiss_toast()` - 15 min

### Phase C: Advanced (3-4 hours total)
- [ ] `calculate_breadcrumb_scroll()` - 45 min
- [ ] `should_show_hotkey()` - 30 min
- [ ] `can_navigate_up()` / `can_navigate_down()` - 25 min
- [ ] `calculate_render_area()` - 40 min

**Total Estimate:** 7-10 hours of focused work over 2-3 weeks

---

## 🔄 Standard Extraction Process

For each function extraction, follow this workflow:

### 1. Identify (5 min)
- Find inline logic in handlers
- Verify it's a pure calculation (no I/O, no mutation beyond return value)
- Note all call sites

### 2. Extract (10-15 min)
- Create pure function in `src/logic/[domain].rs`
- Add comprehensive doc comment
- Include examples if non-obvious

### 3. Test (10-15 min)
- Add 3-5 tests covering:
  - Happy path
  - Edge cases (empty, None, boundary values)
  - Error cases (if applicable)
- Run `cargo test` - all tests must pass

### 4. Replace (5-10 min)
- Replace inline code with function call
- Keep behavior EXACTLY the same
- Verify no logic changes

### 5. Verify (5 min)
- `cargo build` - must compile without errors
- `cargo test` - all tests must pass
- Manual test if user-facing feature

### 6. Commit (5 min)
- Clear commit message following existing format
- Reference function name and locations
- Note "no behavior changes"

**Total per function:** 40-65 minutes

---

## 🎓 Guidelines

### What to Extract

✅ **DO extract:**
- Pure calculations (input → output, no side effects)
- Validation logic (input → bool or Result)
- State transitions (state → new state)
- Data transformations (data → formatted data)
- Business rules (conditions → decision)

❌ **DON'T extract (yet):**
- Async operations (`.await`)
- Direct I/O (API calls, file operations)
- Mutation of App state (beyond simple assignment)
- Complex orchestration (multi-step workflows)

### Naming Conventions

- **Validation:** `can_*`, `should_*`, `is_*`, `has_*`
- **Calculation:** `calculate_*`, `compute_*`, `determine_*`
- **Transformation:** `format_*`, `convert_*`, `map_*`
- **State Transition:** `next_*`, `cycle_*`, `toggle_*`

### Test Patterns

```rust
#[test]
fn test_function_name_happy_path() {
    // Normal, expected case
}

#[test]
fn test_function_name_edge_case() {
    // Boundary values, empty inputs
}

#[test]
fn test_function_name_error_case() {
    // Invalid inputs, error conditions
}
```

---

## 🚫 What NOT to Do

### Anti-Pattern 1: Over-Extraction
```rust
// ❌ BAD: Too granular, not worth extracting
pub fn add_one(x: usize) -> usize {
    x + 1
}
```

### Anti-Pattern 2: Leaky Abstractions
```rust
// ❌ BAD: Function takes entire App, defeats the purpose
pub fn can_delete(app: &App) -> bool {
    app.model.navigation.focus_level > 0
}

// ✅ GOOD: Takes only what it needs
pub fn can_delete(focus_level: usize) -> bool {
    focus_level > 0
}
```

### Anti-Pattern 3: Hidden Side Effects
```rust
// ❌ BAD: Function has side effects (logs, mutates global state)
pub fn calculate_total(items: &[Item]) -> usize {
    log_debug("Calculating total");  // Side effect!
    items.len()
}

// ✅ GOOD: Pure calculation, caller can log
pub fn calculate_total(items: &[Item]) -> usize {
    items.len()
}
```

### Anti-Pattern 4: Premature Optimization
Don't extract functions you *might* need. Only extract what exists and is used.

---

## 📈 Success Metrics

Track progress with these metrics:

- **Tests:** Should increase by ~3 per extraction
- **Coverage:** Aim for 80%+ of extracted logic tested
- **Compilation:** Must always compile without errors
- **Warnings:** Should not increase
- **Behavior:** No user-facing changes until all extractions done

**Current Baseline:**
- 51 tests passing
- 3 warnings (unchanged from before extractions)
- 0 compilation errors

**After Phase A Goal:**
- 65-70 tests passing (+14-19 tests)
- 3 warnings (no increase)
- 0 compilation errors

---

## 🔗 Related Documents

- **ELM_REWRITE_PREP.md** - Overall strategy and philosophy
- **MODEL_DESIGN.md** - Model architecture details
- **PLAN.md** - Original project architecture plan
- **MIGRATION_SUMMARY.md** - Phase 1 & 2 completion summary

---

## 💡 When to Stop

You can pause extractions at any time. Each extraction is independent and provides immediate value.

**Good stopping points:**
1. After Phase A (5 quick wins) - 65-70 tests
2. After Phase B (10 functions total) - 85-95 tests
3. After Phase C (15 functions total) - 110-125 tests

**Don't feel pressured to complete everything!** Even 5 extractions significantly improve testability and code organization.

---

## 🎯 Next Session Checklist

When you return to this work:

1. ✅ Read this document
2. ✅ Check `cargo test` - should be 51 tests passing
3. ✅ Pick Target 1 (can_delete_file) or Target 2 (should_show_restore_button)
4. ✅ Follow standard extraction process
5. ✅ Commit with clear message
6. ✅ Update this document (mark extraction as complete)

**Time estimate for next session:** 30-40 minutes for one extraction

---

*Generated 2025-10-31 after completing first extraction (has_local_changes)*
