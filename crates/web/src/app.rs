use leptos::*;
use leptos_meta::*;
use leptos_router::*;
use serde::{Deserialize, Serialize};

/// A record from the memex daemon
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub record_type: String,
    pub title: String,
    pub description: Option<String>,
    pub content: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/memex-web.css"/>
        <Title text="Memex"/>
        <Router>
            <main class="container">
                <nav>
                    <a href="/">"Records"</a>
                    " | "
                    <a href="/graph">"Graph"</a>
                </nav>
                <Routes>
                    <Route path="/" view=RecordListPage/>
                    <Route path="/record/:id" view=RecordDetailPage/>
                    <Route path="/graph" view=GraphPage/>
                </Routes>
            </main>
        </Router>
    }
}

/// Page showing a list of all records
#[component]
fn RecordListPage() -> impl IntoView {
    let records = create_resource(|| (), |_| async move { fetch_records().await });

    view! {
        <h1>"Records"</h1>
        <Suspense fallback=move || view! { <p>"Loading..."</p> }>
            {move || {
                records
                    .get()
                    .map(|result| match result {
                        Ok(records) => {
                            view! {
                                <RecordList records=records/>
                            }
                                .into_view()
                        }
                        Err(e) => {
                            view! {
                                <p class="error">"Error: " {e}</p>
                            }
                                .into_view()
                        }
                    })
            }}

        </Suspense>
    }
}

/// Component for rendering a list of records
#[component]
fn RecordList(records: Vec<Record>) -> impl IntoView {
    // Group records by type
    let grouped = records.iter().fold(
        std::collections::HashMap::<String, Vec<&Record>>::new(),
        |mut acc, record| {
            acc.entry(record.record_type.clone())
                .or_default()
                .push(record);
            acc
        },
    );

    let mut types: Vec<_> = grouped.keys().cloned().collect();
    types.sort();

    view! {
        <div class="record-list">
            {types
                .into_iter()
                .map(|record_type| {
                    let records = grouped.get(&record_type).unwrap().clone();
                    view! {
                        <section class="record-type-section">
                            <h2>{record_type.clone()}</h2>
                            <ul>
                                {records
                                    .into_iter()
                                    .map(|r| {
                                        let id = r.id.clone();
                                        view! {
                                            <li>
                                                <a href=format!("/record/{}", id)>{r.title.clone()}</a>
                                                {r
                                                    .description
                                                    .as_ref()
                                                    .map(|d| {
                                                        view! {
                                                            <span class="description">" - " {d.clone()}</span>
                                                        }
                                                    })}

                                            </li>
                                        }
                                    })
                                    .collect_view()}

                            </ul>
                        </section>
                    }
                })
                .collect_view()}

        </div>
    }
}

/// Page showing details of a single record
#[component]
fn RecordDetailPage() -> impl IntoView {
    let params = use_params_map();

    let record = create_resource(
        move || params.get().get("id").cloned().unwrap_or_default(),
        |id| async move { fetch_record(&id).await },
    );

    view! {
        <Suspense fallback=move || view! { <p>"Loading..."</p> }>
            {move || {
                record
                    .get()
                    .map(|result| match result {
                        Ok(record) => {
                            view! {
                                <RecordDetail record=record/>
                            }
                                .into_view()
                        }
                        Err(e) => {
                            view! {
                                <p class="error">"Error: " {e}</p>
                            }
                                .into_view()
                        }
                    })
            }}

        </Suspense>
    }
}

/// Component for rendering record details
#[component]
fn RecordDetail(record: Record) -> impl IntoView {
    let content_pretty =
        serde_json::to_string_pretty(&record.content).unwrap_or_else(|_| "{}".to_string());

    view! {
        <article class="record-detail">
            <header>
                <h1>{record.title.clone()}</h1>
                <span class="record-type">{record.record_type.clone()}</span>
            </header>

            {record
                .description
                .as_ref()
                .map(|d| {
                    view! {
                        <p class="description">{d.clone()}</p>
                    }
                })}

            <section class="metadata">
                <p>"Created: " {record.created_at.clone()}</p>
                <p>"Updated: " {record.updated_at.clone()}</p>
            </section>

            <section class="content">
                <h3>"Content"</h3>
                <pre>{content_pretty}</pre>
            </section>

            <a href="/">"Back to list"</a>
        </article>
    }
}

/// Page for graph visualization (placeholder)
#[component]
fn GraphPage() -> impl IntoView {
    view! {
        <h1>"Graph View"</h1>
        <p>"Graph visualization coming soon..."</p>
        <p>"This will show records and their edges as an interactive graph."</p>
    }
}

/// Fetch all records from the daemon
async fn fetch_records() -> Result<Vec<Record>, String> {
    #[cfg(feature = "hydrate")]
    {
        use gloo_net::http::Request;

        let resp = Request::get("/api/records")
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.ok() {
            return Err(format!("HTTP error: {}", resp.status()));
        }

        resp.json().await.map_err(|e| e.to_string())
    }

    #[cfg(not(feature = "hydrate"))]
    {
        // Server-side: return empty for now, will integrate with daemon
        Ok(vec![])
    }
}

/// Fetch a single record by ID
async fn fetch_record(id: &str) -> Result<Record, String> {
    #[cfg(feature = "hydrate")]
    {
        use gloo_net::http::Request;

        let resp = Request::get(&format!("/api/records/{}", id))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.ok() {
            return Err(format!("HTTP error: {}", resp.status()));
        }

        resp.json().await.map_err(|e| e.to_string())
    }

    #[cfg(not(feature = "hydrate"))]
    {
        Err(format!("Record {} not found", id))
    }
}
