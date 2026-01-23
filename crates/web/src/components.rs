//! Leptos components for the Memex web UI

use leptos::*;
use leptos_router::*;

use crate::types::{DashboardStats, Event, MemoView, Record, Task, TaskDetail, Worker};

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
                                            let task_href = format!("/tasks/{}", task_id);
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

    let worker = create_resource(worker_id, |id| async move { fetch_worker(&id).await });

    view! {
        <Layout title="Worker".to_string() active_section="workers".to_string()>
            <Suspense fallback=move || view! { <Loading/> }>
                {move || {
                    worker
                        .get()
                        .map(|result| {
                            match result {
                                Ok(w) => view! { <WorkerDetailContent worker=w/> }.into_view(),
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

#[component]
fn WorkerDetailContent(worker: Worker) -> impl IntoView {
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

                <div class="card transcript-placeholder">
                    <h3>"Transcript"</h3>
                    <p class="card-meta">
                        "Worker transcript is not yet available in the web UI. "
                        "Use " <code>"memex cortex transcript " {&worker.id}</code>
                        " to view the conversation history."
                    </p>
                </div>
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
                            let href = format!("/tasks/{}", task_id);
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
                    view! {
                        <div class="card">
                            <div class="card-title">{r.name}</div>
                            {r
                                .description
                                .map(|d| {
                                    view! { <p>{d}</p> }
                                })}

                            <div class="card-meta">{r.created_at}</div>
                        </div>
                    }
                })
                .collect_view()}

        </div>
    }
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

#[cfg(feature = "hydrate")]
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

#[cfg(not(feature = "hydrate"))]
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

async fn fetch_records_by_type(record_type: &str) -> Result<Vec<Record>, String> {
    fetch_json(&format!("/api/records/{}", record_type)).await
}

async fn fetch_memos() -> Result<Vec<MemoView>, String> {
    fetch_json("/api/memos").await
}

async fn fetch_events() -> Result<Vec<Event>, String> {
    fetch_json("/api/events").await
}
