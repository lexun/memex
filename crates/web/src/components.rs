//! Leptos components for the Memex web UI

use leptos::*;
use leptos_router::*;
use pulldown_cmark::{html, Options, Parser};

#[cfg(any(feature = "hydrate", feature = "csr"))]
use leptos::spawn_local;

// For SSR mode, we need a dummy spawn_local that does nothing
#[cfg(not(any(feature = "hydrate", feature = "csr")))]
fn spawn_local<F>(_future: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
}

use crate::types::{
    ActivityEntry, ActivityFeed, DashboardStats, Event, MemoView, Record, RecordDetail, Task,
    TaskDetail, TranscriptEntry, Worker, WorkerTranscript,
};

// =============================================================================
// Layout Components
// =============================================================================

/// Main layout component with sidebar navigation
#[component]
pub fn Layout(
    /// Page title shown in header
    title: String,
    /// Active section for sidebar highlighting
    #[prop(optional, into)]
    active_section: String,
    /// Page content
    children: Children,
) -> impl IntoView {
    let active = if active_section.is_empty() {
        None
    } else {
        Some(active_section)
    };
    view! {
        <aside class="sidebar">
            <Sidebar active_section=active/>
        </aside>

        <div class="main-wrapper">
            <header>
                <h1>{title}</h1>
            </header>
            <main>{children()}</main>
        </div>
    }
}

/// Sidebar navigation component
#[component]
fn Sidebar(active_section: Option<String>) -> impl IntoView {
    let is_active = move |section: &str| {
        active_section
            .as_ref()
            .map(|s| s.as_str() == section)
            .unwrap_or(false)
    };

    view! {
        <a href="/" class="sidebar-logo">"Memex"</a>

        <div class="sidebar-section">
            <div class="sidebar-section-title">"Work"</div>
            <SidebarLink href="/tasks" icon="\u{2610}" label="Tasks" active=is_active("tasks")/>
            <SidebarLink href="/workers" icon="\u{2699}" label="Workers" active=is_active("workers")/>
            <SidebarLink href="/activity" icon="\u{21BB}" label="Activity" active=is_active("activity")/>
        </div>

        <div class="sidebar-section">
            <div class="sidebar-section-title">"Directory"</div>
            <SidebarLink href="/people" icon="\u{263A}" label="People" active=is_active("people")/>
            <SidebarLink href="/teams" icon="\u{2638}" label="Teams" active=is_active("teams")/>
            <SidebarLink href="/companies" icon="\u{1F3E2}" label="Companies" active=is_active("companies")/>
            <SidebarLink href="/projects" icon="\u{25A3}" label="Projects" active=is_active("projects")/>
            <SidebarLink href="/repos" icon="\u{27E8}\u{27E9}" label="Repos" active=is_active("repos")/>
            <SidebarLink href="/rules" icon="\u{2699}" label="Rules" active=is_active("rules")/>
            <SidebarLink href="/skills" icon="\u{2605}" label="Skills" active=is_active("skills")/>
            <SidebarLink href="/documents" icon="\u{1F4C4}" label="Documents" active=is_active("documents")/>
            <SidebarLink href="/technologies" icon="\u{1F6E0}" label="Technologies" active=is_active("technologies")/>
        </div>

        <div class="sidebar-section">
            <div class="sidebar-section-title">"Activity"</div>
            <SidebarLink href="/memos" icon="\u{270E}" label="Memos" active=is_active("memos")/>
            <SidebarLink href="/threads" icon="\u{2709}" label="Threads" active=is_active("threads")/>
            <SidebarLink href="/events" icon="\u{26A1}" label="Events" active=is_active("events")/>
        </div>
    }
}

/// Helper component for a single sidebar link
#[component]
fn SidebarLink(
    href: &'static str,
    icon: &'static str,
    label: &'static str,
    #[prop(optional)]
    active: bool,
) -> impl IntoView {
    let class_name = if active {
        "sidebar-link active"
    } else {
        "sidebar-link"
    };

    view! {
        <A href=href class=class_name>
            <span class="sidebar-icon">{icon}</span>
            " "
            {label}
        </A>
    }
}

// =============================================================================
// Shared Components
// =============================================================================

/// Status badge component
#[component]
pub fn StatusBadge(
    #[prop(into)] status: String,
    #[prop(optional)] class: Option<String>,
) -> impl IntoView {
    let status_class = status.replace('_', "-");
    let full_class = format!(
        "badge badge-status {}{}",
        status_class,
        class.map(|c| format!(" {}", c)).unwrap_or_default()
    );
    view! { <span class=full_class>{status}</span> }
}

/// Priority badge component
#[component]
pub fn PriorityBadge(priority: i32) -> impl IntoView {
    if priority > 2 {
        return view! {}.into_view();
    }
    let class = format!(
        "badge badge-priority {}",
        match priority {
            0 => "p0",
            1 => "p1",
            2 => "p2",
            _ => "",
        }
    );
    view! { <span class=class>"P" {priority}</span> }.into_view()
}

/// Empty state component
#[component]
pub fn EmptyState(message: &'static str) -> impl IntoView {
    view! {
        <div class="empty-state">
            <p>{message}</p>
        </div>
    }
}

/// Loading state component
#[component]
pub fn Loading() -> impl IntoView {
    view! {
        <div class="loading">
            "Loading"
        </div>
    }
}

/// Markdown rendering component
#[component]
pub fn Markdown(
    /// The markdown content to render
    #[prop(into)]
    content: String,
    /// Optional additional CSS classes
    #[prop(optional, into)]
    class: String,
) -> impl IntoView {
    let html = render_markdown(&content);
    let class_name = if class.is_empty() {
        "markdown-content".to_string()
    } else {
        format!("markdown-content {}", class)
    };

    view! {
        <div class=class_name inner_html=html></div>
    }
}

/// Interactive markdown editor with live preview (Notion-style)
///
/// Features:
/// - Click to edit mode
/// - Live markdown preview while editing
/// - Save/cancel buttons
/// - Keyboard shortcuts (Escape to cancel, Cmd/Ctrl+Enter to save)
#[component]
pub fn MarkdownEditor(
    /// The initial markdown content
    #[prop(into)]
    content: String,
    /// Record ID for saving
    #[prop(into)]
    record_id: String,
    /// Callback when content is saved
    #[prop(optional, into)]
    on_save: Option<Callback<String>>,
) -> impl IntoView {
    let (is_editing, set_is_editing) = create_signal(false);
    let (edit_content, set_edit_content) = create_signal(content.clone());
    let (is_saving, set_is_saving) = create_signal(false);
    let (save_error, set_save_error) = create_signal::<Option<String>>(None);
    let original_content = content.clone();

    // Create derived signal for preview HTML
    let preview_html = move || render_markdown(&edit_content.get());

    // Handle clicking on the content to edit
    let start_editing = move |_| {
        set_is_editing.set(true);
        set_save_error.set(None);
    };

    // Cancel editing and restore original content
    let do_cancel = {
        let original_content = original_content.clone();
        move || {
            set_edit_content.set(original_content.clone());
            set_is_editing.set(false);
            set_save_error.set(None);
        }
    };

    // Save the content
    let do_save = {
        let record_id = record_id.clone();
        let on_save = on_save.clone();
        move || {
            let id = record_id.clone();
            let new_content = edit_content.get();
            let on_save = on_save.clone();

            set_is_saving.set(true);
            set_save_error.set(None);

            spawn_local(async move {
                match save_record_content(&id, &new_content).await {
                    Ok(_) => {
                        set_is_editing.set(false);
                        set_is_saving.set(false);
                        if let Some(callback) = on_save {
                            callback.call(new_content);
                        }
                    }
                    Err(e) => {
                        set_save_error.set(Some(e));
                        set_is_saving.set(false);
                    }
                }
            });
        }
    };

    // Click handlers that wrap the internal functions
    let cancel_editing = {
        let do_cancel = do_cancel.clone();
        move |_: leptos::ev::MouseEvent| {
            do_cancel();
        }
    };

    let save_content = {
        let do_save = do_save.clone();
        move |_: leptos::ev::MouseEvent| {
            do_save();
        }
    };

    // Handle keyboard shortcuts
    let on_keydown = {
        let do_cancel = do_cancel.clone();
        let do_save = do_save.clone();
        move |ev: leptos::ev::KeyboardEvent| {
            let key = ev.key();
            if key == "Escape" {
                do_cancel();
            } else if key == "Enter" && (ev.meta_key() || ev.ctrl_key()) {
                ev.prevent_default();
                do_save();
            }
        }
    };

    view! {
        {move || {
            if is_editing.get() {
                // Edit mode with live preview
                view! {
                    <div class="markdown-editor editing">
                        <div class="editor-container">
                            <div class="editor-pane">
                                <div class="editor-header">
                                    <span class="editor-label">"Edit"</span>
                                    <span class="editor-hint">"Cmd+Enter to save, Escape to cancel"</span>
                                </div>
                                <textarea
                                    class="editor-textarea"
                                    prop:value=move || edit_content.get()
                                    on:input=move |ev| {
                                        set_edit_content.set(event_target_value(&ev));
                                    }
                                    on:keydown=on_keydown.clone()
                                    autofocus=true
                                    placeholder="Write markdown here..."
                                />
                            </div>
                            <div class="preview-pane">
                                <div class="editor-header">
                                    <span class="editor-label">"Preview"</span>
                                </div>
                                <div class="markdown-content" inner_html=preview_html></div>
                            </div>
                        </div>

                        {move || {
                            save_error.get().map(|err| {
                                view! {
                                    <div class="editor-error">
                                        "Error: " {err}
                                    </div>
                                }
                            })
                        }}

                        <div class="editor-actions">
                            <button
                                class="btn btn-secondary"
                                on:click=cancel_editing.clone()
                                disabled=move || is_saving.get()
                            >
                                "Cancel"
                            </button>
                            <button
                                class="btn btn-primary"
                                on:click=save_content.clone()
                                disabled=move || is_saving.get()
                            >
                                {move || if is_saving.get() { "Saving..." } else { "Save" }}
                            </button>
                        </div>
                    </div>
                }.into_view()
            } else {
                // View mode - click to edit
                let html = render_markdown(&edit_content.get());
                view! {
                    <div
                        class="markdown-editor viewable"
                        on:click=start_editing.clone()
                        title="Click to edit"
                    >
                        <div class="edit-hint">
                            <span class="edit-icon">"\u{270E}"</span>
                            " Click to edit"
                        </div>
                        <div class="markdown-content" inner_html=html></div>
                    </div>
                }.into_view()
            }
        }}
    }
}

/// Simple editable text field with inline editing
#[component]
pub fn EditableText(
    /// The current text value
    #[prop(into)]
    value: String,
    /// Record ID for saving
    #[prop(into)]
    record_id: String,
    /// Field name (name, description)
    #[prop(into)]
    field: String,
    /// Placeholder text when empty
    #[prop(optional, into)]
    placeholder: String,
    /// Display as heading (larger text)
    #[prop(optional)]
    heading: bool,
    /// Use textarea for multi-line editing
    #[prop(optional)]
    multiline: bool,
) -> impl IntoView {
    let (is_editing, set_is_editing) = create_signal(false);
    let (edit_value, set_edit_value) = create_signal(value.clone());
    let (is_saving, set_is_saving) = create_signal(false);
    let original_value = value.clone();

    let start_editing = move |_| {
        set_is_editing.set(true);
    };

    let do_cancel = {
        let original_value = original_value.clone();
        move || {
            set_edit_value.set(original_value.clone());
            set_is_editing.set(false);
        }
    };

    let do_save = {
        let record_id = record_id.clone();
        let field = field.clone();
        move || {
            let id = record_id.clone();
            let field = field.clone();
            let new_value = edit_value.get();

            set_is_saving.set(true);

            spawn_local(async move {
                let result = save_record_field(&id, &field, &new_value).await;
                set_is_saving.set(false);
                if result.is_ok() {
                    set_is_editing.set(false);
                }
            });
        }
    };

    // Blur handler to save on focus loss
    let save_on_blur = {
        let do_save = do_save.clone();
        move |_: leptos::ev::FocusEvent| {
            do_save();
        }
    };

    // For single-line: Enter saves, Escape cancels
    // For multi-line: Cmd/Ctrl+Enter saves, Escape cancels
    let on_keydown = {
        let do_cancel = do_cancel.clone();
        let do_save = do_save.clone();
        move |ev: leptos::ev::KeyboardEvent| {
            let key = ev.key();
            if key == "Escape" {
                do_cancel();
            } else if key == "Enter" {
                if multiline {
                    // Multi-line: Cmd/Ctrl+Enter to save
                    if ev.meta_key() || ev.ctrl_key() {
                        ev.prevent_default();
                        do_save();
                    }
                    // Otherwise allow normal Enter for new lines
                } else {
                    // Single-line: Enter to save
                    if !ev.shift_key() {
                        ev.prevent_default();
                        do_save();
                    }
                }
            }
        }
    };

    let class_name = if heading {
        "editable-text editable-heading"
    } else {
        "editable-text"
    };
    let placeholder_text = if placeholder.is_empty() {
        "Click to add...".to_string()
    } else {
        placeholder
    };

    view! {
        {move || {
            if is_editing.get() {
                if multiline {
                    view! {
                        <div class=format!("{} editing multiline", class_name)>
                            <textarea
                                class="editable-textarea"
                                prop:value=move || edit_value.get()
                                on:input=move |ev| {
                                    set_edit_value.set(event_target_value(&ev));
                                }
                                on:keydown=on_keydown.clone()
                                on:blur=save_on_blur.clone()
                                autofocus=true
                                placeholder=placeholder_text.clone()
                                rows=3
                            />
                            <div class="textarea-hint">"Cmd/Ctrl+Enter to save, Esc to cancel"</div>
                            {move || is_saving.get().then(|| view! { <span class="saving-indicator">"..."</span> })}
                        </div>
                    }.into_view()
                } else {
                    view! {
                        <div class=format!("{} editing", class_name)>
                            <input
                                type="text"
                                class="editable-input"
                                prop:value=move || edit_value.get()
                                on:input=move |ev| {
                                    set_edit_value.set(event_target_value(&ev));
                                }
                                on:keydown=on_keydown.clone()
                                on:blur=save_on_blur.clone()
                                autofocus=true
                                placeholder=placeholder_text.clone()
                            />
                            {move || is_saving.get().then(|| view! { <span class="saving-indicator">"..."</span> })}
                        </div>
                    }.into_view()
                }
            } else {
                let display_value = edit_value.get();
                let is_empty = display_value.is_empty();
                view! {
                    <div
                        class=format!("{} viewable{}{}", class_name, if is_empty { " empty" } else { "" }, if multiline { " multiline" } else { "" })
                        on:click=start_editing.clone()
                        title="Click to edit"
                    >
                        {if is_empty {
                            view! { <span class="placeholder">{placeholder_text.clone()}</span> }.into_view()
                        } else if multiline {
                            // Preserve line breaks in multi-line display
                            view! { <span class="multiline-content">{display_value}</span> }.into_view()
                        } else {
                            view! { <span>{display_value}</span> }.into_view()
                        }}
                    </div>
                }.into_view()
            }
        }}
    }
}

/// Render markdown to HTML string
fn render_markdown(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);

    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

/// Stats card component
#[component]
pub fn StatCard(value: usize, label: &'static str) -> impl IntoView {
    view! {
        <div class="stat-card">
            <div class="stat-value">{value}</div>
            <div class="stat-label">{label}</div>
        </div>
    }
}

// =============================================================================
// Dashboard Page
// =============================================================================

/// Dashboard/index page
#[component]
pub fn DashboardPage() -> impl IntoView {
    let stats = create_resource(|| (), |_| async move { fetch_dashboard_stats().await });

    view! {
        <Layout title="Dashboard".to_string()>
            <Suspense fallback=move || view! { <Loading/> }>
                {move || {
                    stats
                        .get()
                        .map(|stats| {
                            let stats = stats.unwrap_or_default();
                            view! {
                                <div class="stats">
                                    <StatCard value=stats.records label="Records"/>
                                    <StatCard value=stats.tasks label="Tasks"/>
                                    <StatCard value=stats.memos label="Memos"/>
                                </div>

                                <h2>"Quick Links"</h2>
                                <div class="card">
                                    <p>
                                        <a href="/tasks">"Manage Tasks"</a>
                                        " - View and manage your tasks"
                                    </p>
                                    <p>
                                        <a href="/workers">"View Workers"</a>
                                        " - Monitor active workers"
                                    </p>
                                    <p>
                                        <a href="/memos">"View Memos"</a>
                                        " - Browse recorded memos"
                                    </p>
                                </div>
                            }
                        })
                }}

            </Suspense>
        </Layout>
    }
}

// =============================================================================
// Tasks Pages
// =============================================================================

/// Sort options for tasks
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TaskSortBy {
    Priority,
    CreatedAt,
    UpdatedAt,
}

/// Tasks list page with filtering and sorting
#[component]
pub fn TasksPage() -> impl IntoView {
    let tasks = create_resource(|| (), |_| async move { fetch_tasks().await });

    // Filter signals
    let (status_filter, set_status_filter) = create_signal(String::from("active")); // "all", "active", or specific status
    let (project_filter, set_project_filter) = create_signal(String::new()); // empty = all projects
    let (priority_filter, set_priority_filter) = create_signal(-1i32); // -1 = all priorities
    let (sort_by, set_sort_by) = create_signal(TaskSortBy::Priority);

    view! {
        <Layout title="Tasks".to_string() active_section="tasks".to_string()>
            <Suspense fallback=move || view! { <Loading/> }>
                {move || {
                    tasks
                        .get()
                        .map(|result| {
                            match result {
                                Ok(all_tasks) => {
                                    // Calculate stats from all tasks
                                    let pending = all_tasks.iter().filter(|t| t.status == "pending").count();
                                    let in_progress = all_tasks
                                        .iter()
                                        .filter(|t| t.status == "in_progress")
                                        .count();
                                    let done = all_tasks
                                        .iter()
                                        .filter(|t| t.status == "done" || t.status == "completed" || t.status == "cancelled")
                                        .count();

                                    // Get unique projects for filter dropdown
                                    let projects: Vec<String> = {
                                        let mut p: Vec<String> = all_tasks
                                            .iter()
                                            .filter_map(|t| t.project.clone())
                                            .collect();
                                        p.sort();
                                        p.dedup();
                                        p
                                    };

                                    view! {
                                        <div class="stats">
                                            <StatCard value=pending label="Pending"/>
                                            <StatCard value=in_progress label="In Progress"/>
                                            <StatCard value=done label="Done"/>
                                        </div>

                                        <TaskFilters
                                            status_filter=status_filter
                                            set_status_filter=set_status_filter
                                            project_filter=project_filter
                                            set_project_filter=set_project_filter
                                            priority_filter=priority_filter
                                            set_priority_filter=set_priority_filter
                                            sort_by=sort_by
                                            set_sort_by=set_sort_by
                                            projects=projects
                                        />

                                        <TasksTableFiltered
                                            tasks=all_tasks
                                            status_filter=status_filter
                                            project_filter=project_filter
                                            priority_filter=priority_filter
                                            sort_by=sort_by
                                        />
                                    }
                                        .into_view()
                                }
                                Err(e) => {
                                    view! { <div class="error">"Error loading tasks: " {e}</div> }
                                        .into_view()
                                }
                            }
                        })
                }}

            </Suspense>
        </Layout>
    }
}

/// Filter controls for tasks
#[component]
fn TaskFilters(
    status_filter: ReadSignal<String>,
    set_status_filter: WriteSignal<String>,
    project_filter: ReadSignal<String>,
    set_project_filter: WriteSignal<String>,
    priority_filter: ReadSignal<i32>,
    set_priority_filter: WriteSignal<i32>,
    sort_by: ReadSignal<TaskSortBy>,
    set_sort_by: WriteSignal<TaskSortBy>,
    projects: Vec<String>,
) -> impl IntoView {
    view! {
        <div class="task-filters">
            <div class="filter-group">
                <label for="status-filter">"Status"</label>
                <select
                    id="status-filter"
                    on:change=move |ev| {
                        set_status_filter.set(event_target_value(&ev));
                    }
                    prop:value=move || status_filter.get()
                >
                    <option value="active">"Active (hide completed)"</option>
                    <option value="all">"All"</option>
                    <option value="pending">"Pending"</option>
                    <option value="in_progress">"In Progress"</option>
                    <option value="blocked">"Blocked"</option>
                    <option value="needs_discussion">"Needs Discussion"</option>
                    <option value="completed">"Completed"</option>
                    <option value="cancelled">"Cancelled"</option>
                </select>
            </div>

            <div class="filter-group">
                <label for="project-filter">"Project"</label>
                <select
                    id="project-filter"
                    on:change=move |ev| {
                        set_project_filter.set(event_target_value(&ev));
                    }
                    prop:value=move || project_filter.get()
                >
                    <option value="">"All Projects"</option>
                    {projects
                        .into_iter()
                        .map(|p| {
                            let p_clone = p.clone();
                            view! { <option value=p>{p_clone}</option> }
                        })
                        .collect_view()}
                </select>
            </div>

            <div class="filter-group">
                <label for="priority-filter">"Priority"</label>
                <select
                    id="priority-filter"
                    on:change=move |ev| {
                        let val = event_target_value(&ev);
                        set_priority_filter.set(val.parse().unwrap_or(-1));
                    }
                    prop:value=move || priority_filter.get().to_string()
                >
                    <option value="-1">"All Priorities"</option>
                    <option value="0">"P0 (Critical)"</option>
                    <option value="1">"P1 (High)"</option>
                    <option value="2">"P2 (Medium)"</option>
                    <option value="3">"P3 (Low)"</option>
                </select>
            </div>

            <div class="filter-group">
                <label for="sort-by">"Sort By"</label>
                <select
                    id="sort-by"
                    on:change=move |ev| {
                        let val = event_target_value(&ev);
                        set_sort_by.set(match val.as_str() {
                            "created" => TaskSortBy::CreatedAt,
                            "updated" => TaskSortBy::UpdatedAt,
                            _ => TaskSortBy::Priority,
                        });
                    }
                    prop:value=move || {
                        match sort_by.get() {
                            TaskSortBy::Priority => "priority",
                            TaskSortBy::CreatedAt => "created",
                            TaskSortBy::UpdatedAt => "updated",
                        }
                    }
                >
                    <option value="priority">"Priority"</option>
                    <option value="created">"Created Date"</option>
                    <option value="updated">"Updated Date"</option>
                </select>
            </div>
        </div>
    }
}

/// Tasks table with filtering and sorting applied
#[component]
fn TasksTableFiltered(
    tasks: Vec<Task>,
    status_filter: ReadSignal<String>,
    project_filter: ReadSignal<String>,
    priority_filter: ReadSignal<i32>,
    sort_by: ReadSignal<TaskSortBy>,
) -> impl IntoView {
    // Create a derived signal for filtered and sorted tasks
    let filtered_tasks = move || {
        let status = status_filter.get();
        let project = project_filter.get();
        let priority = priority_filter.get();
        let sort = sort_by.get();

        let mut filtered: Vec<Task> = tasks
            .iter()
            .filter(|t| {
                // Status filter
                let status_match = match status.as_str() {
                    "all" => true,
                    "active" => t.status != "completed" && t.status != "cancelled" && t.status != "done",
                    s => t.status == s,
                };

                // Project filter
                let project_match = project.is_empty()
                    || t.project.as_ref().map(|p| p == &project).unwrap_or(false);

                // Priority filter
                let priority_match = priority < 0 || t.priority == priority;

                status_match && project_match && priority_match
            })
            .cloned()
            .collect();

        // Sort the filtered tasks
        match sort {
            TaskSortBy::Priority => {
                // Primary: priority (lower = higher priority)
                // Secondary: updated_at (newest first)
                filtered.sort_by(|a, b| {
                    a.priority
                        .cmp(&b.priority)
                        .then_with(|| b.updated_at.cmp(&a.updated_at))
                });
            }
            TaskSortBy::CreatedAt => {
                // Newest first
                filtered.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            }
            TaskSortBy::UpdatedAt => {
                // Most recently updated first
                filtered.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            }
        }

        filtered
    };

    view! {
        {move || {
            let tasks = filtered_tasks();
            view! { <TasksTable tasks=tasks/> }
        }}
    }
}

/// Tasks table component
#[component]
fn TasksTable(tasks: Vec<Task>) -> impl IntoView {
    if tasks.is_empty() {
        return view! { <EmptyState message="No tasks yet."/> }.into_view();
    }

    view! {
        <table>
            <thead>
                <tr>
                    <th>"Title"</th>
                    <th>"Status"</th>
                    <th>"Priority"</th>
                    <th>"Project"</th>
                    <th>"Created"</th>
                </tr>
            </thead>
            <tbody>
                {tasks
                    .into_iter()
                    .map(|task| {
                        let id = task.id.clone();
                        let href = format!("/tasks/{}", id);
                        view! {
                            <tr>
                                <td>
                                    <a href=href>{task.title}</a>
                                </td>
                                <td>
                                    <StatusBadge status=task.status.clone()/>
                                </td>
                                <td>
                                    <PriorityBadge priority=task.priority/>
                                </td>
                                <td>{task.project.unwrap_or_else(|| "-".to_string())}</td>
                                <td>{task.created_at}</td>
                            </tr>
                        }
                    })
                    .collect_view()}

            </tbody>
        </table>
    }
    .into_view()
}

/// Task detail page
#[component]
pub fn TaskDetailPage() -> impl IntoView {
    let params = use_params_map();
    let task_id = move || params.with(|p| p.get("id").cloned().unwrap_or_default());

    let task_detail = create_resource(task_id, |id| async move { fetch_task_detail(&id).await });

    view! {
        <Layout title="Task".to_string() active_section="tasks".to_string()>
            <Suspense fallback=move || view! { <Loading/> }>
                {move || {
                    task_detail
                        .get()
                        .map(|result| {
                            match result {
                                Ok(detail) => view! { <TaskDetailContent detail=detail/> }.into_view(),
                                Err(e) => {
                                    view! {
                                        <div class="detail-header">
                                            <a href="/tasks" class="back-link">
                                                "\u{2190} Back to Tasks"
                                            </a>
                                        </div>
                                        <div class="error">"Error loading task: " {e}</div>
                                    }
                                        .into_view()
                                }
                            }
                        })
                }}

            </Suspense>
        </Layout>
    }
}

#[component]
fn TaskDetailContent(detail: TaskDetail) -> impl IntoView {
    let task = detail.task;
    let notes = detail.notes;
    let workers = detail.assigned_workers;

    view! {
        <div class="detail-header">
            <a href="/tasks" class="back-link">"\u{2190} Back to Tasks"</a>
            <h1 style="margin-top: 0.5rem;">{&task.title}</h1>
            <div class="detail-meta">
                <StatusBadge status=task.status.clone()/>
                <PriorityBadge priority=task.priority/>
                {task
                    .project
                    .as_ref()
                    .map(|p| {
                        view! { <span class="badge badge-project">{p.clone()}</span> }
                    })}

            </div>
        </div>

        <div class="detail-grid">
            <div class="detail-main">
                {task
                    .description
                    .as_ref()
                    .map(|desc| {
                        view! {
                            <div class="card">
                                <h3>"Description"</h3>
                                <p style="white-space: pre-wrap;">{desc.clone()}</p>
                            </div>
                        }
                    })}

                {(!notes.is_empty())
                    .then(|| {
                        view! {
                            <div class="card">
                                <h3>"Notes"</h3>
                                <div class="notes-list">
                                    {notes
                                        .iter()
                                        .map(|note| {
                                            view! {
                                                <div class="note-item">
                                                    <p style="white-space: pre-wrap;">{&note.content}</p>
                                                    <div class="card-meta">{&note.created_at}</div>
                                                </div>
                                            }
                                        })
                                        .collect_view()}

                                </div>
                            </div>
                        }
                    })}

            </div>

            <div class="detail-sidebar">
                <div class="card">
                    <h3>"Details"</h3>
                    <dl class="detail-list">
                        <dt>"ID"</dt>
                        <dd>
                            <code>{&task.id}</code>
                        </dd>
                        <dt>"Status"</dt>
                        <dd>{&task.status}</dd>
                        <dt>"Priority"</dt>
                        <dd>"P" {task.priority}</dd>
                        <dt>"Created"</dt>
                        <dd>{&task.created_at}</dd>
                        <dt>"Updated"</dt>
                        <dd>{&task.updated_at}</dd>
                    </dl>
                </div>

                <div class="card">
                    <h3>"Assigned Workers"</h3>
                    {if workers.is_empty() {
                        view! { <p class="card-meta">"No workers assigned"</p> }.into_view()
                    } else {
                        view! {
                            <ul class="worker-list">
                                {workers
                                    .iter()
                                    .map(|w| {
                                        let href = format!("/workers/{}", w.id);
                                        let state_class = if w.state == "error" { "error" } else { "" };
                                        view! {
                                            <li>
                                                <a href=href>
                                                    <code>{&w.id}</code>
                                                </a>
                                                <span class=format!("badge badge-status {}", state_class)>
                                                    {&w.state}
                                                </span>
                                            </li>
                                        }
                                    })
                                    .collect_view()}

                            </ul>
                        }
                            .into_view()
                    }}

                </div>
            </div>
        </div>
    }
}

// =============================================================================
// Workers Pages
// =============================================================================

/// Workers list page
#[component]
pub fn WorkersPage() -> impl IntoView {
    let workers = create_resource(|| (), |_| async move { fetch_workers().await });

    view! {
        <Layout title="Workers".to_string() active_section="workers".to_string()>
            <Suspense fallback=move || view! { <Loading/> }>
                {move || {
                    workers
                        .get()
                        .map(|result| {
                            match result {
                                Ok(workers) => view! { <WorkersTable workers=workers/> }.into_view(),
                                Err(e) => {
                                    view! { <div class="error">"Error loading workers: " {e}</div> }
                                        .into_view()
                                }
                            }
                        })
                }}

            </Suspense>
        </Layout>
    }
}

#[component]
fn WorkersTable(workers: Vec<Worker>) -> impl IntoView {
    if workers.is_empty() {
        return view! { <EmptyState message="No active workers."/> }.into_view();
    }

    view! {
        <table>
            <thead>
                <tr>
                    <th>"Worker ID"</th>
                    <th>"State"</th>
                    <th>"Current Task"</th>
                    <th>"Worktree"</th>
                    <th>"Messages"</th>
                    <th>"Last Activity"</th>
                </tr>
            </thead>
            <tbody>
                {workers
                    .into_iter()
                    .map(|w| {
                        let href = format!("/workers/{}", w.id);
                        let state_class = if w.state == "error" {
                            "badge badge-error"
                        } else {
                            "badge badge-status"
                        };
                        view! {
                            <tr>
                                <td>
                                    <a href=href>
                                        <code>{&w.id}</code>
                                    </a>
                                </td>
                                <td>
                                    <span class=state_class title=w.error_message.clone()>
                                        {&w.state}
                                    </span>
                                </td>
                                <td>
                                    {w
                                        .current_task
                                        .as_ref()
                                        .map(|task_id| {
                                            // Worker task IDs are Atlas record IDs, not Forge task IDs
                                            let task_href = format!("/records/{}", task_id);
                                            view! {
                                                <a href=task_href>
                                                    <code>{task_id}</code>
                                                </a>
                                            }
                                                .into_view()
                                        })
                                        .unwrap_or_else(|| view! { "-" }.into_view())}

                                </td>
                                <td>
                                    {w
                                        .worktree
                                        .as_ref()
                                        .map(|path| {
                                            let truncated = if path.len() > 40 {
                                                format!("...{}", &path[path.len() - 37..])
                                            } else {
                                                path.clone()
                                            };
                                            view! {
                                                <code title=path.clone()>{truncated}</code>
                                            }
                                                .into_view()
                                        })
                                        .unwrap_or_else(|| view! { "-" }.into_view())}

                                </td>
                                <td>{w.messages_sent} " / " {w.messages_received}</td>
                                <td>
                                    <time>{&w.last_activity}</time>
                                </td>
                            </tr>
                        }
                    })
                    .collect_view()}

            </tbody>
        </table>
    }
    .into_view()
}

/// Worker detail page
#[component]
pub fn WorkerDetailPage() -> impl IntoView {
    let params = use_params_map();
    let worker_id = move || params.with(|p| p.get("id").cloned().unwrap_or_default());

    let worker = create_resource(worker_id.clone(), |id| async move { fetch_worker(&id).await });

    // Create a signal for the transcript that SSE will update
    let (transcript, set_transcript) = create_signal::<Option<WorkerTranscript>>(None);

    // Set up SSE connection for real-time transcript updates (client-side only)
    #[cfg(any(feature = "hydrate", feature = "csr"))]
    {
        use wasm_bindgen::prelude::*;
        use wasm_bindgen::JsCast;

        // Use create_effect with a guard to only set up SSE once
        let (sse_initialized, set_sse_initialized) = create_signal(false);

        create_effect(move |_| {
            // Only initialize SSE once
            if sse_initialized.get() {
                return;
            }

            let id = worker_id();
            if id.is_empty() {
                return;
            }

            set_sse_initialized.set(true);

            use web_sys::{EventSource, MessageEvent};

            let url = format!("/api/workers/{}/transcript/stream", id);
            let es = EventSource::new(&url).ok();
            if let Some(event_source) = es.clone() {
                let onmessage =
                    Closure::<dyn Fn(MessageEvent)>::new(move |event: MessageEvent| {
                        if let Some(data) = event.data().as_string() {
                            if let Ok(transcript_data) =
                                serde_json::from_str::<WorkerTranscript>(&data)
                            {
                                set_transcript.set(Some(transcript_data));
                            }
                        }
                    });
                event_source.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
                onmessage.forget();

                let onerror = Closure::<dyn Fn()>::new(move || {
                    leptos::logging::log!("SSE connection error - will auto-reconnect");
                });
                event_source.set_onerror(Some(onerror.as_ref().unchecked_ref()));
                onerror.forget();

                leptos::logging::log!("SSE connection established for worker transcript");
            }

            // Clean up EventSource on unmount
            on_cleanup(move || {
                if let Some(event_source) = es {
                    event_source.close();
                    leptos::logging::log!("SSE connection closed");
                }
            });
        });
    }

    view! {
        <Layout title="Worker".to_string() active_section="workers".to_string()>
            <Suspense fallback=move || view! { <Loading/> }>
                {move || {
                    worker
                        .get()
                        .map(|result| {
                            match result {
                                Ok(w) => {
                                    let transcript_view = transcript.get();
                                    view! { <WorkerDetailContent worker=w transcript=transcript_view/> }
                                        .into_view()
                                }
                                Err(e) => {
                                    view! {
                                        <div class="detail-header">
                                            <a href="/workers" class="back-link">
                                                "\u{2190} Back to Workers"
                                            </a>
                                        </div>
                                        <div class="error">"Error loading worker: " {e}</div>
                                    }
                                        .into_view()
                                }
                            }
                        })
                }}

            </Suspense>
        </Layout>
    }
}

/// Transcript view component showing conversation history
#[component]
fn TranscriptView(
    transcript: Option<WorkerTranscript>,
    #[prop(optional)] _worker_id: String,
) -> impl IntoView {
    match transcript {
        Some(t) if !t.entries.is_empty() => {
            let source_label = match t.source.as_str() {
                "database" => "persistent",
                "memory" => "in-memory",
                _ => &t.source,
            };
            let entry_count = t.entries.len();
            view! {
                <div class="card transcript-card">
                    <div class="transcript-header">
                        <h3>"Transcript"</h3>
                        <span class="transcript-meta">
                            {entry_count} " entries \u{2022} " {source_label.to_string()} " \u{2022} "
                            <span class="live-indicator">"live"</span>
                        </span>
                    </div>
                    <div class="transcript-entries">
                        {t
                            .entries
                            .into_iter()
                            .enumerate()
                            .map(|(i, entry)| {
                                view! { <TranscriptEntryView entry=entry index=i/> }
                            })
                            .collect_view()}

                    </div>
                </div>
            }
                .into_view()
        }
        _ => view! {
            <div class="card transcript-placeholder">
                <h3>"Transcript"</h3>
                <p class="card-meta">"Connecting to live updates..."</p>
            </div>
        }
            .into_view(),
    }
}

/// Single transcript entry view
#[component]
fn TranscriptEntryView(entry: TranscriptEntry, index: usize) -> impl IntoView {
    // Format the timestamp to show time only
    let time_str = entry
        .timestamp
        .split('T')
        .nth(1)
        .and_then(|t| t.split('.').next())
        .unwrap_or(&entry.timestamp)
        .to_string();

    // Show full prompts and responses without truncation
    let prompt_preview = entry.prompt.clone();
    let response_preview = entry.response.clone();

    let duration_str = if entry.duration_ms > 0 {
        format!("{}ms", entry.duration_ms)
    } else {
        "pending".to_string()
    };

    let entry_class = if entry.is_error {
        "transcript-entry transcript-error"
    } else {
        "transcript-entry"
    };

    view! {
        <div class=entry_class>
            <div class="transcript-entry-header">
                <span class="transcript-entry-number">"#" {index + 1}</span>
                <span class="transcript-entry-time">{time_str}</span>
                <span class="transcript-entry-duration">{duration_str}</span>
            </div>
            <div class="transcript-prompt">
                <span class="transcript-label">"Prompt:"</span>
                <pre class="transcript-content">{prompt_preview}</pre>
            </div>
            {response_preview
                .map(|resp| {
                    view! {
                        <div class="transcript-response">
                            <span class=if entry.is_error {
                                "transcript-label transcript-error-label"
                            } else {
                                "transcript-label"
                            }>
                                {if entry.is_error { "Error:" } else { "Response:" }}
                            </span>
                            <pre class="transcript-content">{resp}</pre>
                        </div>
                    }
                })}

        </div>
    }
}

#[component]
fn WorkerDetailContent(
    worker: Worker,
    transcript: Option<WorkerTranscript>,
) -> impl IntoView {
    let state_class = worker.state_class();

    view! {
        <div class="detail-header">
            <a href="/workers" class="back-link">"\u{2190} Back to Workers"</a>
            <h1 style="margin-top: 0.5rem;">
                "Worker " <code>{&worker.id}</code>
            </h1>
            <div class="detail-meta">
                <span class=format!("badge badge-status {}", state_class)>{&worker.state}</span>
                {worker
                    .model
                    .as_ref()
                    .map(|m| view! { <span class="badge badge-model">{m.clone()}</span> })}

            </div>
        </div>

        {worker
            .error_message
            .as_ref()
            .map(|e| {
                view! {
                    <div class="error-banner">
                        <strong>"Error:"</strong>
                        " "
                        {e.clone()}
                    </div>
                }
            })}

        <div class="detail-grid">
            <div class="detail-main">
                <div class="card">
                    <h3>"Status"</h3>
                    <div class="status-grid">
                        <div class="status-item">
                            <div class="status-value">{worker.messages_sent}</div>
                            <div class="status-label">"Messages Sent"</div>
                        </div>
                        <div class="status-item">
                            <div class="status-value">{worker.messages_received}</div>
                            <div class="status-label">"Responses Received"</div>
                        </div>
                    </div>
                </div>

                {worker
                    .worktree
                    .as_ref()
                    .map(|path| {
                        view! {
                            <div class="card">
                                <h3>"Worktree"</h3>
                                <code class="path-display">{path.clone()}</code>
                            </div>
                        }
                    })}

                <div class="card">
                    <h3>"Working Directory"</h3>
                    <code class="path-display">{&worker.cwd}</code>
                </div>

                <TranscriptView transcript=transcript _worker_id=worker.id.clone()/>
            </div>

            <div class="detail-sidebar">
                <div class="card">
                    <h3>"Details"</h3>
                    <dl class="detail-list">
                        <dt>"Worker ID"</dt>
                        <dd>
                            <code>{&worker.id}</code>
                        </dd>
                        <dt>"State"</dt>
                        <dd>{&worker.state}</dd>
                        {worker
                            .model
                            .as_ref()
                            .map(|m| {
                                view! {
                                    <dt>"Model"</dt>
                                    <dd>{m.clone()}</dd>
                                }
                            })}

                        <dt>"Started"</dt>
                        <dd>{&worker.started_at}</dd>
                        <dt>"Last Activity"</dt>
                        <dd>{&worker.last_activity}</dd>
                    </dl>
                </div>

                <div class="card">
                    <h3>"Current Task"</h3>
                    {worker
                        .current_task
                        .as_ref()
                        .map(|task_id| {
                            // Worker task IDs are Atlas record IDs, not Forge task IDs
                            let href = format!("/records/{}", task_id);
                            view! {
                                <a href=href class="task-link">
                                    <code>{task_id.clone()}</code>
                                    <span class="link-arrow">"\u{2192}"</span>
                                </a>
                            }
                                .into_view()
                        })
                        .unwrap_or_else(|| {
                            view! { <p class="card-meta">"No task assigned"</p> }.into_view()
                        })}

                </div>
            </div>
        </div>
    }
}

/// Worker activity page showing combined transcript from all workers
#[component]
pub fn WorkerActivityPage() -> impl IntoView {
    // Create a signal for refresh trigger
    #[allow(unused_variables)]
    let (refresh_count, set_refresh_count) = create_signal(0u32);

    // Create activity feed resource that depends on refresh_count
    let activity = create_resource(
        move || refresh_count.get(),
        |_| async move { fetch_activity_feed().await },
    );

    // Auto-refresh every 5 seconds when component is mounted
    #[cfg(any(feature = "hydrate", feature = "csr"))]
    {
        use std::time::Duration;
        let handle = set_interval_with_handle(
            move || {
                set_refresh_count.update(|n| *n = n.wrapping_add(1));
            },
            Duration::from_secs(5),
        );
        on_cleanup(move || {
            if let Ok(h) = handle {
                h.clear();
            }
        });
    }

    view! {
        <Layout title="Worker Activity".to_string() active_section="activity".to_string()>
            <Suspense fallback=move || view! { <Loading/> }>
                {move || {
                    activity
                        .get()
                        .map(|result| {
                            match result {
                                Ok(feed) => view! { <ActivityFeedView feed=feed/> }.into_view(),
                                Err(e) => {
                                    view! { <div class="error">"Error loading activity: " {e}</div> }
                                        .into_view()
                                }
                            }
                        })
                }}

            </Suspense>
        </Layout>
    }
}

/// Activity feed view component
#[component]
fn ActivityFeedView(feed: ActivityFeed) -> impl IntoView {
    if feed.entries.is_empty() {
        return view! {
            <EmptyState message="No worker activity yet. Workers will appear here when they start processing messages."/>
        }
        .into_view();
    }

    view! {
        <div class="activity-feed">
            <div class="activity-header">
                <span class="activity-count">{feed.entries.len()} " recent entries"</span>
                <span class="activity-live">"Live updates enabled"</span>
            </div>
            <div class="activity-entries">
                {feed
                    .entries
                    .into_iter()
                    .map(|entry| view! { <ActivityEntryView entry=entry/> })
                    .collect_view()}

            </div>
        </div>
    }
    .into_view()
}

/// Single activity entry view
#[component]
fn ActivityEntryView(entry: ActivityEntry) -> impl IntoView {
    // Format the timestamp to show time only
    let time_str = entry
        .timestamp
        .split('T')
        .nth(1)
        .and_then(|t| t.split('.').next())
        .unwrap_or(&entry.timestamp)
        .to_string();

    // Show full prompts and responses without truncation
    let prompt_preview = entry.prompt.clone();
    let response_preview = entry.response.clone();

    let duration_str = if entry.duration_ms > 0 {
        format!("{}ms", entry.duration_ms)
    } else {
        "pending".to_string()
    };

    let entry_class = if entry.is_error {
        "activity-entry activity-error"
    } else {
        "activity-entry"
    };

    let worker_href = format!("/workers/{}", entry.worker_id);

    view! {
        <div class=entry_class>
            <div class="activity-entry-header">
                <a href=worker_href class="activity-worker-link">
                    <code>{&entry.worker_id}</code>
                </a>
                <span class=format!("badge badge-status {}", entry.worker_state)>
                    {&entry.worker_state}
                </span>
                {entry
                    .current_task
                    .as_ref()
                    .map(|task_id| {
                        // Worker task IDs are Atlas record IDs, not Forge task IDs
                        let task_href = format!("/records/{}", task_id);
                        view! {
                            <a href=task_href class="activity-task-link">
                                "Task: " <code>{task_id.clone()}</code>
                            </a>
                        }
                    })}

                <span class="activity-time">{time_str}</span>
                <span class="activity-duration">{duration_str}</span>
            </div>
            <div class="activity-prompt">
                <pre class="activity-content">{prompt_preview}</pre>
            </div>
            {response_preview
                .map(|resp| {
                    view! {
                        <div class="activity-response">
                            <pre class=if entry.is_error {
                                "activity-content activity-error-content"
                            } else {
                                "activity-content"
                            }>{resp}</pre>
                        </div>
                    }
                })}

        </div>
    }
}

// =============================================================================
// Directory Pages
// =============================================================================

/// Generic record list page component
#[component]
pub fn RecordListPage(
    title: &'static str,
    section: &'static str,
    record_type: &'static str,
    empty_message: &'static str,
) -> impl IntoView {
    let records = create_resource(
        move || record_type.to_string(),
        |rt| async move { fetch_records_by_type(&rt).await },
    );

    view! {
        <Layout title=title.to_string() active_section=section.to_string()>
            <Suspense fallback=move || view! { <Loading/> }>
                {move || {
                    records
                        .get()
                        .map(|result| {
                            match result {
                                Ok(records) => {
                                    if records.is_empty() {
                                        view! { <EmptyState message=empty_message/> }.into_view()
                                    } else {
                                        view! { <RecordCardGrid records=records/> }.into_view()
                                    }
                                }
                                Err(e) => {
                                    view! { <div class="error">"Error loading records: " {e}</div> }
                                        .into_view()
                                }
                            }
                        })
                }}

            </Suspense>
        </Layout>
    }
}

#[component]
fn RecordCardGrid(records: Vec<Record>) -> impl IntoView {
    view! {
        <div class="card-grid">
            {records
                .into_iter()
                .map(|r| {
                    let href = format!("/records/{}", r.id);
                    view! {
                        <a href=href class="card card-link">
                            <div class="card-title">{r.name}</div>
                            {r
                                .description
                                .map(|d| {
                                    view! { <p>{d}</p> }
                                })}

                            <div class="card-meta">{r.created_at}</div>
                        </a>
                    }
                })
                .collect_view()}

        </div>
    }
}

/// Generic record detail page component
#[component]
pub fn RecordDetailPage() -> impl IntoView {
    let params = use_params_map();
    let record_id = move || params.with(|p| p.get("id").cloned().unwrap_or_default());

    let record_detail = create_resource(record_id, |id| async move { fetch_record_detail(&id).await });

    view! {
        <Layout title="Record".to_string() active_section="".to_string()>
            <Suspense fallback=move || view! { <Loading/> }>
                {move || {
                    record_detail
                        .get()
                        .map(|result| {
                            match result {
                                Ok(detail) => view! { <RecordDetailContent detail=detail/> }.into_view(),
                                Err(e) => {
                                    view! {
                                        <div class="detail-header">
                                            <a href="javascript:history.back()" class="back-link">
                                                "\u{2190} Back"
                                            </a>
                                        </div>
                                        <div class="error">"Error loading record: " {e}</div>
                                    }
                                        .into_view()
                                }
                            }
                        })
                }}

            </Suspense>
        </Layout>
    }
}

#[component]
fn RecordDetailContent(detail: RecordDetail) -> impl IntoView {
    let record = detail.record;
    let related = detail.related;

    // Get back link based on record type
    let back_link = format!("/{}", get_plural_section(&record.record_type));
    let type_badge_class = format!("badge badge-type {}", record.record_type);

    // Extract markdown content from the JSON content field
    let content_markdown = extract_content_markdown(&record.content);
    let record_id = record.id.clone();
    let record_id_for_name = record.id.clone();
    let record_id_for_desc = record.id.clone();

    view! {
        <div class="detail-header">
            <a href=back_link class="back-link">"\u{2190} Back"</a>
            <div style="margin-top: 0.5rem;">
                <EditableText
                    value=record.name.clone()
                    record_id=record_id_for_name
                    field="name"
                    placeholder="Record name"
                    heading=true
                />
            </div>
            <div class="detail-meta">
                <span class=type_badge_class>{&record.record_type}</span>
            </div>
        </div>

        <div class="detail-grid">
            <div class="detail-main">
                <div class="card">
                    <h3>"Description"</h3>
                    <EditableText
                        value=record.description.clone().unwrap_or_default()
                        record_id=record_id_for_desc
                        field="description"
                        placeholder="Add a description..."
                        multiline=true
                    />
                </div>

                <div class="card">
                    <h3>"Content"</h3>
                    <MarkdownEditor
                        content=content_markdown.clone().unwrap_or_default()
                        record_id=record_id.clone()
                    />
                </div>

                {(!related.is_empty())
                    .then(|| {
                        view! {
                            <div class="card">
                                <h3>"Related Records"</h3>
                                <div class="related-records">
                                    {related
                                        .iter()
                                        .map(|r| {
                                            let href = format!("/records/{}", r.id);
                                            let type_class = format!("badge badge-type {}", r.record_type);
                                            view! {
                                                <a href=href class="related-record-link">
                                                    <span class=type_class>{&r.record_type}</span>
                                                    " "
                                                    {&r.name}
                                                </a>
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            </div>
                        }
                    })}
            </div>

            <div class="detail-sidebar">
                <div class="card">
                    <h3>"Details"</h3>
                    <dl class="detail-list">
                        <dt>"ID"</dt>
                        <dd>
                            <code>{&record.id}</code>
                        </dd>
                        <dt>"Type"</dt>
                        <dd>{&record.record_type}</dd>
                        <dt>"Created"</dt>
                        <dd>{&record.created_at}</dd>
                        <dt>"Updated"</dt>
                        <dd>{&record.updated_at}</dd>
                    </dl>
                </div>
            </div>
        </div>
    }
}

/// Get plural section name for back link
fn get_plural_section(record_type: &str) -> &'static str {
    match record_type {
        "person" => "people",
        "team" => "teams",
        "company" => "companies",
        "project" => "projects",
        "repo" => "repos",
        "rule" => "rules",
        "skill" => "skills",
        "document" => "documents",
        "technology" => "technologies",
        _ => "records",
    }
}

/// Extract markdown content from the JSON content field
fn extract_content_markdown(content: &serde_json::Value) -> Option<String> {
    // Try to extract as string first (for simple text content)
    if let Some(s) = content.as_str() {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }

    // Try to extract a "text" or "content" field from an object
    if let Some(obj) = content.as_object() {
        // Try common field names for markdown content
        for field in &["text", "content", "body", "markdown", "description"] {
            if let Some(s) = obj.get(*field).and_then(|v| v.as_str()) {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }

        // If no known field, format as pretty JSON for display
        if !obj.is_empty() {
            return Some(format!("```json\n{}\n```", serde_json::to_string_pretty(&content).unwrap_or_default()));
        }
    }

    // For arrays, format as JSON
    if let Some(arr) = content.as_array() {
        if !arr.is_empty() {
            return Some(format!("```json\n{}\n```", serde_json::to_string_pretty(&content).unwrap_or_default()));
        }
    }

    None
}

/// People page
#[component]
pub fn PeoplePage() -> impl IntoView {
    view! { <RecordListPage title="People" section="people" record_type="person" empty_message="No people yet."/> }
}

/// Teams page
#[component]
pub fn TeamsPage() -> impl IntoView {
    view! { <RecordListPage title="Teams" section="teams" record_type="team" empty_message="No teams yet."/> }
}

/// Companies page
#[component]
pub fn CompaniesPage() -> impl IntoView {
    view! { <RecordListPage title="Companies" section="companies" record_type="company" empty_message="No companies yet."/> }
}

/// Projects page
#[component]
pub fn ProjectsPage() -> impl IntoView {
    view! { <RecordListPage title="Projects" section="projects" record_type="project" empty_message="No projects yet."/> }
}

/// Repos page
#[component]
pub fn ReposPage() -> impl IntoView {
    let records = create_resource(|| (), |_| async move { fetch_records_by_type("repo").await });

    view! {
        <Layout title="Repositories".to_string() active_section="repos".to_string()>
            <Suspense fallback=move || view! { <Loading/> }>
                {move || {
                    records
                        .get()
                        .map(|result| {
                            match result {
                                Ok(records) => {
                                    if records.is_empty() {
                                        view! { <EmptyState message="No repositories yet."/> }.into_view()
                                    } else {
                                        view! { <ReposTable records=records/> }.into_view()
                                    }
                                }
                                Err(e) => {
                                    view! { <div class="error">"Error loading repos: " {e}</div> }
                                        .into_view()
                                }
                            }
                        })
                }}

            </Suspense>
        </Layout>
    }
}

#[component]
fn ReposTable(records: Vec<Record>) -> impl IntoView {
    view! {
        <table>
            <thead>
                <tr>
                    <th>"Name"</th>
                    <th>"Description"</th>
                    <th>"Created"</th>
                </tr>
            </thead>
            <tbody>
                {records
                    .into_iter()
                    .map(|r| {
                        view! {
                            <tr>
                                <td>{r.name}</td>
                                <td>{r.description.unwrap_or_else(|| "-".to_string())}</td>
                                <td>{r.created_at}</td>
                            </tr>
                        }
                    })
                    .collect_view()}

            </tbody>
        </table>
    }
}

/// Rules page
#[component]
pub fn RulesPage() -> impl IntoView {
    view! { <RecordListPage title="Rules" section="rules" record_type="rule" empty_message="No rules yet."/> }
}

/// Skills page
#[component]
pub fn SkillsPage() -> impl IntoView {
    view! { <RecordListPage title="Skills" section="skills" record_type="skill" empty_message="No skills yet."/> }
}

/// Documents page
#[component]
pub fn DocumentsPage() -> impl IntoView {
    view! { <RecordListPage title="Documents" section="documents" record_type="document" empty_message="No documents yet."/> }
}

/// Technologies page
#[component]
pub fn TechnologiesPage() -> impl IntoView {
    view! { <RecordListPage title="Technologies" section="technologies" record_type="technology" empty_message="No technologies yet."/> }
}

// =============================================================================
// Activity Pages
// =============================================================================

/// Memos page
#[component]
pub fn MemosPage() -> impl IntoView {
    let memos = create_resource(|| (), |_| async move { fetch_memos().await });

    view! {
        <Layout title="Memos".to_string() active_section="memos".to_string()>
            <Suspense fallback=move || view! { <Loading/> }>
                {move || {
                    memos
                        .get()
                        .map(|result| {
                            match result {
                                Ok(memos) => {
                                    if memos.is_empty() {
                                        view! { <EmptyState message="No memos yet."/> }.into_view()
                                    } else {
                                        view! {
                                            <div class="card-grid">
                                                {memos
                                                    .into_iter()
                                                    .map(|m| {
                                                        view! {
                                                            <div class="card">
                                                                <p style="white-space: pre-wrap;">{m.content}</p>
                                                                <div class="card-meta">
                                                                    <span class="badge badge-event">{m.source}</span>
                                                                    " "
                                                                    {m.created_at}
                                                                </div>
                                                            </div>
                                                        }
                                                    })
                                                    .collect_view()}

                                            </div>
                                        }
                                            .into_view()
                                    }
                                }
                                Err(e) => {
                                    view! { <div class="error">"Error loading memos: " {e}</div> }
                                        .into_view()
                                }
                            }
                        })
                }}

            </Suspense>
        </Layout>
    }
}

/// Threads page (placeholder)
#[component]
pub fn ThreadsPage() -> impl IntoView {
    view! {
        <Layout title="Threads".to_string() active_section="threads".to_string()>
            <EmptyState message="Coming soon."/>
        </Layout>
    }
}

/// Events page
#[component]
pub fn EventsPage() -> impl IntoView {
    let events = create_resource(|| (), |_| async move { fetch_events().await });

    view! {
        <Layout title="Events".to_string() active_section="events".to_string()>
            <Suspense fallback=move || view! { <Loading/> }>
                {move || {
                    events
                        .get()
                        .map(|result| {
                            match result {
                                Ok(events) => {
                                    if events.is_empty() {
                                        view! { <EmptyState message="No events yet."/> }.into_view()
                                    } else {
                                        view! {
                                            <div class="card-grid">
                                                {events
                                                    .into_iter()
                                                    .map(|e| {
                                                        view! {
                                                            <div class="card">
                                                                <div class="card-title">
                                                                    <span class="badge badge-event">{e.event_type}</span>
                                                                </div>
                                                                {e.summary.map(|s| view! { <p>{s}</p> })}
                                                                <div class="card-meta">
                                                                    {e.source}
                                                                    " \u{2022} "
                                                                    {e.timestamp}
                                                                </div>
                                                            </div>
                                                        }
                                                    })
                                                    .collect_view()}

                                            </div>
                                        }
                                            .into_view()
                                    }
                                }
                                Err(e) => {
                                    view! { <div class="error">"Error loading events: " {e}</div> }
                                        .into_view()
                                }
                            }
                        })
                }}

            </Suspense>
        </Layout>
    }
}

// =============================================================================
// API Fetching Functions
// =============================================================================

#[cfg(any(feature = "hydrate", feature = "csr"))]
async fn fetch_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    use gloo_net::http::Request;

    let resp = Request::get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }

    resp.json().await.map_err(|e| e.to_string())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
async fn fetch_json<T: serde::de::DeserializeOwned>(_url: &str) -> Result<T, String> {
    Err("SSR mode - data should be provided by server".to_string())
}

async fn fetch_dashboard_stats() -> Result<DashboardStats, String> {
    fetch_json("/api/stats").await
}

async fn fetch_tasks() -> Result<Vec<Task>, String> {
    fetch_json("/api/tasks").await
}

async fn fetch_task_detail(id: &str) -> Result<TaskDetail, String> {
    fetch_json(&format!("/api/tasks/{}", id)).await
}

async fn fetch_workers() -> Result<Vec<Worker>, String> {
    fetch_json("/api/workers").await
}

async fn fetch_worker(id: &str) -> Result<Worker, String> {
    fetch_json(&format!("/api/workers/{}", id)).await
}

async fn fetch_activity_feed() -> Result<ActivityFeed, String> {
    fetch_json("/api/activity").await
}

async fn fetch_records_by_type(record_type: &str) -> Result<Vec<Record>, String> {
    fetch_json(&format!("/api/records/{}", record_type)).await
}

async fn fetch_record_detail(id: &str) -> Result<RecordDetail, String> {
    fetch_json(&format!("/api/record/{}", id)).await
}

async fn fetch_memos() -> Result<Vec<MemoView>, String> {
    fetch_json("/api/memos").await
}

async fn fetch_events() -> Result<Vec<Event>, String> {
    fetch_json("/api/events").await
}

/// Save record content via API
#[cfg(any(feature = "hydrate", feature = "csr"))]
async fn save_record_content(id: &str, content: &str) -> Result<(), String> {
    use gloo_net::http::Request;

    let body = serde_json::json!({
        "content": content
    });

    let resp = Request::put(&format!("/api/record/{}/content", id))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }

    Ok(())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
async fn save_record_content(_id: &str, _content: &str) -> Result<(), String> {
    Err("SSR mode - cannot save".to_string())
}

/// Save a specific field of a record
#[cfg(any(feature = "hydrate", feature = "csr"))]
async fn save_record_field(id: &str, field: &str, value: &str) -> Result<(), String> {
    use gloo_net::http::Request;

    let body = serde_json::json!({
        field: value
    });

    let resp = Request::put(&format!("/api/record/{}", id))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }

    Ok(())
}

#[cfg(not(any(feature = "hydrate", feature = "csr")))]
async fn save_record_field(_id: &str, _field: &str, _value: &str) -> Result<(), String> {
    Err("SSR mode - cannot save".to_string())
}
