use leptos::*;
use leptos_router::*;

/// Main layout component with sidebar navigation
#[component]
pub fn Layout(
    /// Page title shown in header
    title: String,
    /// Active section for sidebar highlighting
    #[prop(optional)]
    active_section: Option<String>,
    /// Page content
    children: Children,
) -> impl IntoView {
    view! {
        <aside class="sidebar">
            {match active_section {
                Some(section) => view! { <Sidebar active_section=section/> }.into_view(),
                None => view! { <Sidebar/> }.into_view(),
            }}
        </aside>

        <div class="main-wrapper">
            <header>
                <h1>{title}</h1>
            </header>
            <main>{children()}</main>
        </div>
    }
}

/// Helper component for a single sidebar link
#[component]
fn SidebarLink(
    href: &'static str,
    icon: &'static str,
    label: &'static str,
    #[prop(optional)] active: bool,
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

/// Sidebar navigation component
#[component]
fn Sidebar(
    #[prop(optional)] active_section: Option<String>,
) -> impl IntoView {
    let is_active = |section: &str| {
        active_section.as_ref().map(|s| s.as_str() == section).unwrap_or(false)
    };

    view! {
        <a href="/" class="sidebar-logo">
            "Memex"
        </a>

        <div class="sidebar-section">
            <div class="sidebar-section-title">"Work"</div>
            <SidebarLink href="/tasks" icon="☐" label="Tasks" active=is_active("tasks")/>
            <SidebarLink href="/workers" icon="⚙" label="Workers" active=is_active("workers")/>
        </div>

        <div class="sidebar-section">
            <div class="sidebar-section-title">"Directory"</div>
            <SidebarLink href="/people" icon="☺" label="People" active=is_active("people")/>
            <SidebarLink href="/teams" icon="☸" label="Teams" active=is_active("teams")/>
            <SidebarLink href="/companies" icon="🏢" label="Companies" active=is_active("companies")/>
            <SidebarLink href="/projects" icon="▣" label="Projects" active=is_active("projects")/>
            <SidebarLink href="/repos" icon="⟨⟩" label="Repos" active=is_active("repos")/>
            <SidebarLink href="/rules" icon="⚙" label="Rules" active=is_active("rules")/>
            <SidebarLink href="/skills" icon="★" label="Skills" active=is_active("skills")/>
            <SidebarLink href="/documents" icon="📄" label="Documents" active=is_active("documents")/>
            <SidebarLink href="/technologies" icon="🛠" label="Technologies" active=is_active("technologies")/>
        </div>

        <div class="sidebar-section">
            <div class="sidebar-section-title">"Activity"</div>
            <SidebarLink href="/memos" icon="✎" label="Memos" active=is_active("memos")/>
            <SidebarLink href="/threads" icon="✉" label="Threads" active=is_active("threads")/>
            <SidebarLink href="/events" icon="⚡" label="Events" active=is_active("events")/>
        </div>
    }
}

/// Dashboard/index page
#[component]
pub fn DashboardPage() -> impl IntoView {
    // TODO: Fetch actual stats from daemon
    let record_count = 0;
    let task_count = 0;
    let memo_count = 0;

    view! {
        <Layout title="Dashboard".to_string()>
            <div class="stats">
                <div class="stat-card">
                    <div class="stat-value">{record_count}</div>
                    <div class="stat-label">"Records"</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">{task_count}</div>
                    <div class="stat-label">"Tasks"</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">{memo_count}</div>
                    <div class="stat-label">"Memos"</div>
                </div>
            </div>

            <h2>"Quick Links"</h2>
            <div class="card">
                <p>
                    <a href="/records">"Browse Records"</a>
                    " - View all records in your knowledge base"
                </p>
                <p>
                    <a href="/tasks">"Manage Tasks"</a>
                    " - View and manage your tasks"
                </p>
                <p>
                    <a href="/memos">"View Memos"</a>
                    " - Browse recorded memos"
                </p>
            </div>
        </Layout>
    }
}

/// Tasks list page
#[component]
pub fn TasksPage() -> impl IntoView {
    // TODO: Fetch actual tasks from daemon
    let pending_count = 0;
    let in_progress_count = 0;
    let done_count = 0;
    let tasks: Vec<()> = vec![];

    view! {
        <Layout title="Tasks".to_string() active_section="tasks".to_string()>
            <div class="stats">
                <div class="stat-card">
                    <div class="stat-value">{pending_count}</div>
                    <div class="stat-label">"Pending"</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">{in_progress_count}</div>
                    <div class="stat-label">"In Progress"</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">{done_count}</div>
                    <div class="stat-label">"Done"</div>
                </div>
            </div>

            {if tasks.is_empty() {
                view! {
                    <div class="empty-state">
                        <p>"No tasks yet."</p>
                    </div>
                }
                    .into_view()
            } else {
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
                        // TODO: Render tasks
                        </tbody>
                    </table>
                }
                    .into_view()
            }}

        </Layout>
    }
}
