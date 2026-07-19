use crate::config::Config;
use crate::detectors::Finding;
use crate::learning::store::LearningStore;
use anyhow::Result;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
enum SeverityFilter {
    All,
    Blocking,
    Warning,
    Info,
}

impl SeverityFilter {
    fn matches(&self, finding: &Finding) -> bool {
        match self {
            SeverityFilter::All => true,
            SeverityFilter::Blocking => finding.severity == "blocking",
            SeverityFilter::Warning => finding.severity == "warning",
            SeverityFilter::Info => finding.severity == "info",
        }
    }
    fn next(&self) -> Self {
        match self {
            SeverityFilter::All => SeverityFilter::Blocking,
            SeverityFilter::Blocking => SeverityFilter::Warning,
            SeverityFilter::Warning => SeverityFilter::Info,
            SeverityFilter::Info => SeverityFilter::All,
        }
    }
    fn label(&self) -> &'static str {
        match self {
            SeverityFilter::All => "All",
            SeverityFilter::Blocking => "Blocking",
            SeverityFilter::Warning => "Warning",
            SeverityFilter::Info => "Info",
        }
    }
    fn icon(&self) -> &'static str {
        match self {
            SeverityFilter::All => "",
            SeverityFilter::Blocking => "✗ ",
            SeverityFilter::Warning => "⚠ ",
            SeverityFilter::Info => "ℹ ",
        }
    }
}

struct FileGroup {
    path: String,
    finding_indices: Vec<usize>,
}

struct TuiItem {
    finding: Finding,
    dismissed: bool,
}

struct AppState {
    items: Vec<TuiItem>,
    filtered: Vec<usize>,
    filter: SeverityFilter,
    selected: usize,
    detail_expanded: bool,
    help_visible: bool,
    dismissed_count: usize,
    /// Cached counts (recalculated only on state changes, not every frame)
    active_count: usize,
    blocking_count: usize,
    warning_count: usize,
    info_count: usize,
    copied_flash: Option<Instant>,

    // Smooth scroll
    display_scroll: usize,
    target_scroll: usize,

    // Micro-animations
    detail_pulse_at: Option<Instant>,
    dismissing: Vec<(usize, Instant)>, // (item_idx, started_at)

    // Cache for grouped findings (computed after rebuild_filtered)
    groups: Vec<FileGroup>,
}

fn build_items(findings: &crate::detectors::Findings) -> Vec<TuiItem> {
    let active: Vec<Finding> = LearningStore::open()
        .ok()
        .and_then(|store| store.filter_findings(&findings.findings).ok())
        .unwrap_or_else(|| findings.findings.clone());

    let active_fingerprints: std::collections::HashSet<String> =
        active.iter().map(|f| f.fingerprint()).collect();

    findings
        .findings
        .iter()
        .map(|f| TuiItem {
            finding: f.clone(),
            dismissed: !active_fingerprints.contains(&f.fingerprint()),
        })
        .collect()
}

fn rebuild_filtered(state: &mut AppState) {
    state.filtered = state
        .items
        .iter()
        .enumerate()
        .filter(|(i, item)| {
            !item.dismissed
                && state.filter.matches(&item.finding)
                && !state.dismissing.iter().any(|(di, _)| *di == *i)
        })
        .map(|(i, _)| i)
        .collect();

    let max = state.filtered.len().saturating_sub(1);
    state.selected = state.selected.min(max);
    state.target_scroll = state.target_scroll.min(max.saturating_sub(1));

    // Recalculate cached counts
    state.active_count = 0;
    state.blocking_count = 0;
    state.warning_count = 0;
    state.info_count = 0;
    for item in &state.items {
        if !item.dismissed {
            state.active_count += 1;
            match item.finding.severity {
                "blocking" => state.blocking_count += 1,
                "warning" => state.warning_count += 1,
                _ => state.info_count += 1,
            }
        }
    }
}

fn rebuild_groups(state: &mut AppState) {
    let mut map: std::collections::BTreeMap<String, Vec<usize>> = std::collections::BTreeMap::new();
    for &idx in &state.filtered {
        if let Some(v) = map.get_mut(&state.items[idx].finding.file) {
            v.push(idx);
        } else {
            map.insert(state.items[idx].finding.file.clone(), vec![idx]);
        }
    }
    state.groups = map
        .into_iter()
        .map(|(path, finding_indices)| FileGroup {
            path,
            finding_indices,
        })
        .collect();
}

fn resolve_editor(config: &Config) -> String {
    if let Some(ref editor) = config.tui.editor {
        return editor.clone();
    }
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vim".to_string())
}

fn open_in_editor(item: &TuiItem, config: &Config) {
    let editor = resolve_editor(config);
    let file = &item.finding.file;
    let line = item.finding.line;

    let result = if line > 0 {
        std::process::Command::new(&editor)
            .arg(format!("{file}:{line}"))
            .status()
    } else {
        std::process::Command::new(&editor).arg(file).status()
    };
    if let Err(e) = result {
        eprintln!("Warning: failed to open editor '{editor}': {e}");
    }
}

fn gen_ai_prompt(item: &TuiItem) -> String {
    let finding = &item.finding;
    format!(
        "Codasaurus found an issue in `{file}`{line}\n\
         \n\
         **Detector:** {detector}  \n\
         **Severity:** {severity}  \n\
         **Message:** {message}{suggestion}\n\
         \n\
         Can you help fix this?",
        file = finding.file,
        line = if finding.line > 0 {
            format!(":{}", finding.line)
        } else {
            String::new()
        },
        detector = finding.detector,
        severity = finding.severity,
        message = finding.message,
        suggestion = finding
            .suggestion
            .as_ref()
            .map(|s| format!("\n**Suggested fix:** {s}"))
            .unwrap_or_default(),
    )
}

fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write;

    let copy_cmd = |cmd: &mut std::process::Command| -> bool {
        if let Ok(mut child) = cmd.spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                if let Err(e) = stdin.write_all(text.as_bytes()) {
                    eprintln!("Warning: failed to write to clipboard: {e}");
                }
            }
            // stdin is dropped here, sending EOF to the subprocess
            child.wait().map(|s| s.success()).unwrap_or(false)
        } else {
            false
        }
    };

    let copied = copy_cmd(
        std::process::Command::new("pbcopy")
            .arg("-")
            .stdin(std::process::Stdio::piped()),
    ) || copy_cmd(
        std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped()),
    ) || copy_cmd(
        std::process::Command::new("xsel")
            .args(["-i", "-b"])
            .stdin(std::process::Stdio::piped()),
    );

    if !copied {
        eprintln!("Prompt (not copied, install pbcopy/xclip/xsel):\n{text}");
    }
    copied
}

fn dismiss_finding(item: &TuiItem) {
    if let Ok(store) = LearningStore::open() {
        if let Err(e) = store.dismiss(&item.finding) {
            eprintln!("Warning: failed to persist dismissal: {e}");
        }
    }
}

/// RAII guard that restores terminal raw mode on drop.
/// Prevents leaving the terminal in a broken state if the TUI panics.
struct RawModeGuard;

impl RawModeGuard {
    fn enter() -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), crossterm::cursor::Show);
    }
}

pub fn run(findings: &crate::detectors::Findings, config: &Config) -> Result<()> {
    let items = build_items(findings);
    if items.is_empty() {
        println!("  ✓  No issues found");
        return Ok(());
    }

    let _guard = RawModeGuard::enter()?;
    let mut stdout = std::io::stdout();
    let backend = CrosstermBackend::new(&mut stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = run_tui(&mut terminal, items, config);

    println!();
    result
}

fn run_tui(
    terminal: &mut Terminal<CrosstermBackend<&mut std::io::Stdout>>,
    items: Vec<TuiItem>,
    config: &Config,
) -> Result<()> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind};

    let mut state = AppState {
        items,
        filtered: Vec::new(),
        filter: SeverityFilter::All,
        selected: 0,
        detail_expanded: false,
        help_visible: false,
        dismissed_count: 0,
        active_count: 0,
        blocking_count: 0,
        warning_count: 0,
        info_count: 0,
        copied_flash: None,
        display_scroll: 0,
        target_scroll: 0,
        detail_pulse_at: None,
        dismissing: Vec::new(),
        groups: Vec::new(),
    };
    rebuild_filtered(&mut state);
    rebuild_groups(&mut state);

    let frame_duration = Duration::from_millis(16); // ~60 fps max

    // Only re-draw when something actually changed (events, animation state).
    // This prevents burning CPU at 60fps on an idle terminal.
    let mut dirty = true;

    loop {
        if dirty {
            terminal.draw(|f| render(f, &state))?;
            dirty = false;
        }

        // Advance smooth scroll each frame (only dirty if scroll changed)
        let prev_scroll = state.display_scroll;
        advance_scroll(&mut state);
        if state.display_scroll != prev_scroll {
            dirty = true;
        }

        // Process completed dismiss animations
        if process_dismissals(&mut state) {
            dirty = true;
        }

        // Check if copied flash expired
        if let Some(t) = state.copied_flash {
            if t.elapsed() > Duration::from_secs(2) {
                state.copied_flash = None;
                dirty = true;
            }
        }

        // Check if detail pulse expired
        if let Some(t) = state.detail_pulse_at {
            if t.elapsed() > Duration::from_millis(120) {
                state.detail_pulse_at = None;
                dirty = true;
            }
        }

        // Poll for events with a short timeout
        if !event::poll(frame_duration)? {
            continue;
        }

        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('j') | KeyCode::Down => {
                    if !state.filtered.is_empty() {
                        state.selected = (state.selected + 1).min(state.filtered.len() - 1);
                        state.detail_pulse_at = Some(Instant::now());
                        snap_scroll_to_selected(&mut state);
                        dirty = true;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if !state.filtered.is_empty() {
                        state.selected = state.selected.saturating_sub(1);
                        state.detail_pulse_at = Some(Instant::now());
                        snap_scroll_to_selected(&mut state);
                        dirty = true;
                    }
                }
                KeyCode::Enter => {
                    state.detail_expanded = !state.detail_expanded;
                    dirty = true;
                }
                KeyCode::Char('o') => {
                    if let Some(item) = selected_item(&state) {
                        if !item.dismissed {
                            open_in_editor(item, config);
                        }
                    }
                }
                KeyCode::Char('d') => {
                    let item_idx = match state.filtered.get(state.selected) {
                        Some(&idx) => idx,
                        None => continue,
                    };
                    if !state.items[item_idx].dismissed
                        && !state.dismissing.iter().any(|(di, _)| *di == item_idx)
                    {
                        state.dismissing.push((item_idx, Instant::now()));
                        dirty = true;
                    }
                }
                KeyCode::Char('p') => {
                    if let Some(item) = selected_item(&state) {
                        if !item.dismissed {
                            let prompt = gen_ai_prompt(item);
                            copy_to_clipboard(&prompt);
                            state.copied_flash = Some(Instant::now());
                            dirty = true;
                        }
                    }
                }
                KeyCode::Char('b') => {
                    state.filter = SeverityFilter::Blocking;
                    rebuild_filtered(&mut state);
                    rebuild_groups(&mut state);
                    state.detail_pulse_at = Some(Instant::now());
                    dirty = true;
                }
                KeyCode::Char('w') => {
                    state.filter = SeverityFilter::Warning;
                    rebuild_filtered(&mut state);
                    rebuild_groups(&mut state);
                    state.detail_pulse_at = Some(Instant::now());
                    dirty = true;
                }
                KeyCode::Char('i') => {
                    state.filter = SeverityFilter::Info;
                    rebuild_filtered(&mut state);
                    rebuild_groups(&mut state);
                    state.detail_pulse_at = Some(Instant::now());
                    dirty = true;
                }
                KeyCode::Char('a') => {
                    state.filter = SeverityFilter::All;
                    rebuild_filtered(&mut state);
                    rebuild_groups(&mut state);
                    state.detail_pulse_at = Some(Instant::now());
                    dirty = true;
                }
                KeyCode::Tab => {
                    state.filter = state.filter.next();
                    rebuild_filtered(&mut state);
                    rebuild_groups(&mut state);
                    state.detail_pulse_at = Some(Instant::now());
                    dirty = true;
                }
                KeyCode::Char('?') => {
                    state.help_visible = !state.help_visible;
                    dirty = true;
                }
                _ => {}
            },
            Event::Resize(_, _) => {}
            _ => {}
        }
    }

    if state.dismissed_count > 0 {
        println!(
            "  {} finding(s) dismissed, will be hidden on future runs.",
            state.dismissed_count
        );
    }

    Ok(())
}

fn selected_item(state: &AppState) -> Option<&TuiItem> {
    state
        .filtered
        .get(state.selected)
        .map(|&idx| &state.items[idx])
}

fn snap_scroll_to_selected(state: &mut AppState) {
    let total_visible = state.filtered.len().saturating_sub(1);
    if state.selected == 0 {
        state.target_scroll = 0;
    } else if state.selected >= total_visible.saturating_sub(1) {
        state.target_scroll = state.filtered.len().saturating_sub(1);
    } else {
        state.target_scroll = state.selected.saturating_sub(1);
    }
}

fn advance_scroll(state: &mut AppState) {
    if state.display_scroll < state.target_scroll {
        state.display_scroll += 1;
    } else if state.display_scroll > state.target_scroll {
        state.display_scroll = state.display_scroll.saturating_sub(1);
    }
}

/// Returns true if any dismissals were processed (caller should re-draw).
fn process_dismissals(state: &mut AppState) -> bool {
    let mut changed = false;
    state.dismissing.retain(|(item_idx, started_at)| {
        if started_at.elapsed() > Duration::from_millis(250) {
            state.items[*item_idx].dismissed = true;
            state.dismissed_count += 1;
            dismiss_finding(&state.items[*item_idx]);
            changed = true;
            false
        } else {
            true
        }
    });
    if changed {
        rebuild_filtered(state);
        rebuild_groups(state);
        snap_scroll_to_selected(state);
    }
    changed
}

fn render(f: &mut Frame, state: &AppState) {
    if state.help_visible {
        render_help(f, f.area());
        return;
    }
    render_main(f, f.area(), state);
}

fn render_main(f: &mut Frame, area: Rect, state: &AppState) {
    let header_height = 1;

    let chunks = Layout::vertical([
        Constraint::Length(header_height), // header
        Constraint::Length(1),             // separator
        Constraint::Min(1),                // content area (left list + right detail)
        Constraint::Length(1),             // separator
        Constraint::Length(1),             // footer
    ])
    .split(area);

    render_header(f, chunks[0], state);
    render_separator(f, chunks[1]);

    // Content area: left pane (findings list) + right pane (detail)
    let content_chunks = Layout::horizontal([
        Constraint::Percentage(40),
        Constraint::Length(1), // gap
        Constraint::Percentage(59),
    ])
    .split(chunks[2]);

    render_findings_list(f, content_chunks[0], state);
    render_detail_pane(f, content_chunks[2], state);

    render_separator(f, chunks[3]);
    render_footer(f, chunks[4], state);
}

fn render_header(f: &mut Frame, area: Rect, state: &AppState) {
    let total = state.items.len();
    let active = state.active_count;
    let blocking = state.blocking_count;
    let warning = state.warning_count;
    let info = state.info_count;
    let dimmed = total - active;

    let filters = [
        (SeverityFilter::All, active),
        (SeverityFilter::Blocking, blocking),
        (SeverityFilter::Warning, warning),
        (SeverityFilter::Info, info),
    ];

    let mut spans: Vec<Span> = Vec::new();

    // Left: app name + filter tabs
    spans.push(Span::styled(
        " codasaurus ",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    ));

    for (i, (filt, count)) in filters.iter().enumerate() {
        let is_active = *filt == state.filter;
        let label = if *filt == SeverityFilter::All {
            format!(" [{}:{}] ", filt.label(), count)
        } else {
            format!(" [{}{}:{}] ", filt.icon(), filt.label(), count)
        };

        let style = if is_active {
            Style::default()
                .fg(match filt {
                    SeverityFilter::All => Color::White,
                    SeverityFilter::Blocking => Color::Red,
                    SeverityFilter::Warning => Color::Yellow,
                    SeverityFilter::Info => Color::Cyan,
                })
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(label, style));
    }

    // Right: dismissed count
    if dimmed > 0 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!("[{dimmed} dismissed]"),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_separator(f: &mut Frame, area: Rect) {
    let width = area.width as usize;
    let line = "─".repeat(width.saturating_sub(1));
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            line,
            Style::default().fg(Color::DarkGray),
        ))),
        area,
    );
}

fn render_findings_list(f: &mut Frame, area: Rect, state: &AppState) {
    let mut rows: Vec<ListItem> = Vec::new();
    let mut row_index = 0;
    let visible_start = state.display_scroll;
    let mut in_view = false;

    for fg in &state.groups {
        let active_count = fg
            .finding_indices
            .iter()
            .filter(|&&idx| !state.items[idx].dismissed)
            .count();
        if active_count == 0 {
            continue;
        }

        // File header row
        if row_index >= visible_start {
            let header_label = format!(" ◆ {}  ", fg.path);
            rows.push(ListItem::new(Line::from(vec![
                Span::styled(
                    header_label,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{active_count}"),
                    Style::default().fg(Color::DarkGray),
                ),
            ])));
            in_view = true;
        }
        row_index += 1;
        if !in_view && row_index > visible_start {
            in_view = true;
        }

        for &idx in &fg.finding_indices {
            if state.items[idx].dismissed {
                continue;
            }
            if row_index >= visible_start {
                rows.push(build_finding_row(idx, &state.items[idx], state));
                in_view = true;
            }
            row_index += 1;
            if !in_view && row_index > visible_start {
                in_view = true;
            }
        }
    }

    if rows.is_empty() {
        let msg = if state.filtered.is_empty() {
            "  No findings match this filter"
        } else {
            "  All findings dismissed"
        };
        rows.push(ListItem::new(Line::from(Span::styled(
            msg,
            Style::default().fg(Color::DarkGray),
        ))));
    }

    // Trim to visible area
    let max_rows = area.height as usize;
    if rows.len() > max_rows {
        rows.truncate(max_rows);
    }

    f.render_widget(List::new(rows), area);
}

fn build_finding_row<'a>(idx: usize, item: &'a TuiItem, state: &AppState) -> ListItem<'a> {
    let f = &item.finding;

    let sev_char = match f.severity {
        "blocking" => "✗",
        "warning" => "⚠",
        _ => "ℹ",
    };

    let sev_color = match f.severity {
        "blocking" => Color::Red,
        "warning" => Color::Yellow,
        _ => Color::Cyan,
    };

    let is_selected = state.filtered.get(state.selected) == Some(&idx);
    let is_dismissing = state.dismissing.iter().any(|(di, _)| *di == idx);

    let bg = if is_selected {
        Style::default().bg(Color::Rgb(35, 35, 45))
    } else {
        Style::default()
    };

    let location = if f.line > 0 {
        format!(":{}", f.line)
    } else {
        String::new()
    };

    // Truncate message to fit
    let msg_max = 40usize;
    let msg_display = if f.message.len() > msg_max {
        format!("{}…", f.message.chars().take(msg_max).collect::<String>())
    } else {
        f.message.clone()
    };

    ListItem::new(
        Line::from(vec![
            Span::styled(
                format!(" {sev_char} "),
                (if is_dismissing {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(sev_color)
                })
                .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}{}", f.detector, location),
                Style::default().fg(Color::Rgb(120, 180, 240)),
            ),
            Span::raw("  "),
            Span::styled(
                msg_display,
                if is_dismissing {
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ])
        .style(bg),
    )
}

fn render_detail_pane(f: &mut Frame, area: Rect, state: &AppState) {
    let item = match selected_item(state) {
        Some(item) => item,
        None => {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "  No findings",
                    Style::default().fg(Color::DarkGray),
                ))),
                area,
            );
            return;
        }
    };

    let finding = &item.finding;

    let pulse_active = state
        .detail_pulse_at
        .map(|t| t.elapsed() < Duration::from_millis(120))
        .unwrap_or(false);

    // Left accent bar color
    let accent_color = if pulse_active {
        Color::Cyan
    } else {
        Color::Rgb(50, 50, 60)
    };

    // Build detail content
    let mut lines: Vec<Line> = Vec::new();

    // Severity badge line
    let (sev_icon, sev_color) = match finding.severity {
        "blocking" => (" ✗ BLOCKING ", Color::Red),
        "warning" => (" ⚠ WARNING ", Color::Yellow),
        _ => (" ℹ INFO ", Color::Cyan),
    };

    lines.push(Line::from(vec![
        Span::styled(
            sev_icon,
            Style::default()
                .fg(Color::Black)
                .bg(sev_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            &finding.detector,
            Style::default()
                .fg(Color::Rgb(120, 180, 240))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(&finding.file, Style::default().fg(Color::DarkGray)),
        if finding.line > 0 {
            Span::styled(
                format!(":{}", finding.line),
                Style::default().fg(Color::DarkGray),
            )
        } else {
            Span::raw("")
        },
    ]));

    lines.push(Line::from(""));

    // Full message
    lines.push(Line::from(Span::styled(
        &finding.message,
        Style::default().fg(Color::White),
    )));
    lines.push(Line::from(""));

    // Evidence / code context
    if let Some(ref evidence) = finding.evidence {
        lines.push(Line::from(Span::styled(
            " evidence",
            Style::default().fg(Color::DarkGray),
        )));
        for ev_line in evidence.lines() {
            let display = if ev_line.len() > 60 {
                format!(" ┊ {}…", ev_line.chars().take(57).collect::<String>())
            } else {
                format!(" ┊ {ev_line}")
            };
            lines.push(Line::from(Span::styled(
                display,
                Style::default().fg(Color::Rgb(160, 160, 160)),
            )));
        }
        lines.push(Line::from(""));
    }

    // Suggestion + codemod (shown only when detail_expanded)
    if state.detail_expanded {
        if let Some(ref suggestion) = finding.suggestion {
            lines.push(Line::from(Span::styled(
                " fix",
                Style::default().fg(Color::Rgb(100, 200, 150)),
            )));
            for sug_line in suggestion.lines() {
                let display = if sug_line.len() > 60 {
                    format!(" {}", sug_line.chars().take(58).collect::<String>())
                } else {
                    format!(" {sug_line}")
                };
                lines.push(Line::from(Span::styled(
                    display,
                    Style::default().fg(Color::Rgb(140, 220, 180)),
                )));
            }
            lines.push(Line::from(""));
        }

        if let Some(ref codemod) = finding.codemod {
            lines.push(Line::from(Span::styled(
                " codemod",
                Style::default().fg(Color::Rgb(180, 130, 220)),
            )));
            lines.push(Line::from(Span::styled(
                format!(" $ {codemod}"),
                Style::default().fg(Color::Rgb(200, 160, 240)),
            )));
            lines.push(Line::from(""));
        }
    }

    // Trim lines to fit available height
    let max_lines = area.height as usize;
    if lines.len() > max_lines {
        lines.truncate(max_lines);
    }

    // Render with left accent bar
    let detail_para = Paragraph::new(Text::from(lines)).style(Style::default());

    // We render a one-cell left border manually via a separate paragraph
    // Actually, let's keep it simple — just use a Block with a LEFT border
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(accent_color));
    f.render_widget(detail_para.block(block), area);
}

fn render_footer(f: &mut Frame, area: Rect, state: &AppState) {
    // Copied flash
    if let Some(ref flash) = state.copied_flash {
        if flash.elapsed() < Duration::from_secs(2) {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    " ✓ Copied! Paste into your AI agent/IDE.",
                    Style::default().fg(Color::Green),
                ))),
                area,
            );
            return;
        }
    }

    let total = state.filtered.len();
    let pos = if state.filtered.is_empty() {
        0
    } else {
        state.selected + 1
    };

    let pos_str = format!(" {pos}/{total} ");

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(&pos_str, Style::default().fg(Color::Rgb(120, 120, 140))),
            Span::styled("↑↓ j/k", Style::default().fg(Color::DarkGray)),
            Span::styled(" nav ", Style::default().fg(Color::Rgb(80, 80, 90))),
            Span::styled("Enter", Style::default().fg(Color::DarkGray)),
            if state.detail_expanded {
                Span::styled(" v", Style::default().fg(Color::Rgb(80, 80, 90)))
            } else {
                Span::styled(" ▸", Style::default().fg(Color::Rgb(80, 80, 90)))
            },
            Span::styled(" o ", Style::default().fg(Color::DarkGray)),
            Span::styled("open ", Style::default().fg(Color::Rgb(80, 80, 90))),
            Span::styled("d ", Style::default().fg(Color::DarkGray)),
            Span::styled("dismiss ", Style::default().fg(Color::Rgb(80, 80, 90))),
            Span::styled("p ", Style::default().fg(Color::DarkGray)),
            Span::styled("AI ", Style::default().fg(Color::Rgb(80, 80, 90))),
            Span::styled("?", Style::default().fg(Color::DarkGray)),
            Span::styled(" help ", Style::default().fg(Color::Rgb(80, 80, 90))),
            Span::styled("q", Style::default().fg(Color::DarkGray)),
            Span::styled(" quit", Style::default().fg(Color::Rgb(80, 80, 90))),
        ])),
        area,
    );
}

fn render_help(f: &mut Frame, area: Rect) {
    let text = Text::from(vec![
        Line::from(Span::styled(
            " Help",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::raw("  ↑/↓   j/k       Navigate findings")),
        Line::from(Span::raw(
            "  Enter             Toggle expanded view (suggestion + codemod)",
        )),
        Line::from(Span::raw("  o                 Open file in editor")),
        Line::from(Span::raw("  d                 Dismiss finding (persisted)")),
        Line::from(Span::raw(
            "  p                 Copy AI fix prompt to clipboard",
        )),
        Line::from(Span::raw(
            "  a   b   w   i     Filter: All / Blocking / Warning / Info",
        )),
        Line::from(Span::raw("  Tab               Cycle through filters")),
        Line::from(Span::raw("  ?                 Toggle this help")),
        Line::from(Span::raw("  q   Esc           Quit")),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            "  Press 'p' on a finding to copy a ready-made prompt",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "  you can paste into your AI agent/IDE.",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::raw("")),
        Line::from(Span::styled(
            "  Press any key to close.",
            Style::default().fg(Color::DarkGray),
        )),
    ]);

    let para = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    let inner = Rect {
        x: area.x + 4,
        y: area.y + 2,
        width: area.width.saturating_sub(8),
        height: area.height.saturating_sub(4),
    };
    f.render_widget(para, inner);
}
