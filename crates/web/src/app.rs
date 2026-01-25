use leptos::*;
use leptos_meta::*;
use leptos_router::*;
use crate::components::*;

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/memex-web.css"/>
        <Link rel="stylesheet" href="/style/main.css"/>
        <Title text="Memex"/>

        <Router>
            <Routes>
                // Home/Dashboard
                <Route path="/" view=DashboardPage/>

                // Work section
                <Route path="/tasks" view=TasksPage/>
                <Route path="/tasks/:id" view=TaskDetailPage/>
                <Route path="/workers" view=WorkersPage/>
                <Route path="/workers/:id" view=WorkerDetailPage/>
                <Route path="/activity" view=WorkerActivityPage/>

                // Directory section
                <Route path="/people" view=PeoplePage/>
                <Route path="/teams" view=TeamsPage/>
                <Route path="/companies" view=CompaniesPage/>
                <Route path="/projects" view=ProjectsPage/>
                <Route path="/repos" view=ReposPage/>
                <Route path="/rules" view=RulesPage/>
                <Route path="/skills" view=SkillsPage/>
                <Route path="/documents" view=DocumentsPage/>
                <Route path="/technologies" view=TechnologiesPage/>
                <Route path="/records/:id" view=RecordDetailPage/>

                // Activity section
                <Route path="/memos" view=MemosPage/>
                <Route path="/threads" view=ThreadsPage/>
                <Route path="/events" view=EventsPage/>
            </Routes>
        </Router>
    }
}

#[cfg(feature = "ssr")]
pub fn shell(_options: LeptosOptions) -> impl IntoView {
    view! {
        <App/>
    }
}
