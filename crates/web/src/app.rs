use leptos::*;
use leptos_meta::*;
use leptos_router::*;
use crate::components::{DashboardPage, TasksPage};

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/memex-web.css"/>
        <Link rel="stylesheet" href="/style/main.css"/>
        <Title text="Memex"/>

        <Router>
            <Routes>
                <Route path="/" view=DashboardPage/>
                <Route path="/tasks" view=TasksPage/>
                // TODO: Add other routes (workers, people, teams, etc.)
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
