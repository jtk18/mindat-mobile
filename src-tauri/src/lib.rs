use mindat_rs::{LocalitiesQuery, MindatClient};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// Application state
pub struct AppState {
    client: RwLock<Option<Arc<MindatClient>>>,
    api_key: RwLock<Option<String>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            client: RwLock::new(None),
            api_key: RwLock::new(None),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Error type for commands
#[derive(Debug, Serialize)]
pub struct CommandError {
    message: String,
}

impl From<mindat_rs::MindatError> for CommandError {
    fn from(e: mindat_rs::MindatError) -> Self {
        CommandError {
            message: e.to_string(),
        }
    }
}

impl From<&str> for CommandError {
    fn from(s: &str) -> Self {
        CommandError {
            message: s.to_string(),
        }
    }
}

impl From<String> for CommandError {
    fn from(s: String) -> Self {
        CommandError { message: s }
    }
}

type CommandResult<T> = Result<T, CommandError>;

/// Get or create the API client
async fn get_client(state: &AppState) -> CommandResult<Arc<MindatClient>> {
    let client = state.client.read().await;
    if let Some(c) = client.as_ref() {
        return Ok(Arc::clone(c));
    }
    drop(client);

    let api_key = state.api_key.read().await;
    if let Some(key) = api_key.as_ref() {
        let new_client = Arc::new(MindatClient::new(key));
        let mut client_write = state.client.write().await;
        *client_write = Some(Arc::clone(&new_client));
        return Ok(new_client);
    }

    Err("API key not configured. Please set your Mindat API key.".into())
}

/// Set the API key
#[tauri::command]
async fn set_api_key(state: State<'_, AppState>, key: String) -> CommandResult<bool> {
    if key.trim().is_empty() {
        return Err("API key cannot be empty".into());
    }

    // Store the key
    let mut api_key = state.api_key.write().await;
    *api_key = Some(key.clone());
    drop(api_key);

    // Create new client
    let new_client = Arc::new(MindatClient::new(&key));
    let mut client = state.client.write().await;
    *client = Some(new_client);

    Ok(true)
}

/// Check if API key is configured
#[tauri::command]
async fn is_configured(state: State<'_, AppState>) -> CommandResult<bool> {
    let api_key = state.api_key.read().await;
    Ok(api_key.is_some())
}

/// Search localities by GPS coordinates
/// Supports dual search: name filter OR elements filter (runs both and merges)
#[tauri::command]
async fn search_localities_nearby(
    state: State<'_, AppState>,
    latitude: f64,
    longitude: f64,
    radius_km: f64,
    country: Option<String>,
    name_contains: Option<String>,
    elements_filter: Option<String>,
    start_page_name: Option<i32>,
    start_page_elements: Option<i32>,
    pages_to_fetch: Option<i32>,
) -> CommandResult<serde_json::Value> {
    // Require country filter
    if country.is_none() {
        return Err("Country is required to narrow down the search".into());
    }

    let client = get_client(&state).await?;

    // Calculate bounding box
    let lat_delta = radius_km / 111.0;
    let lon_delta = radius_km / (111.0 * latitude.to_radians().cos().abs().max(0.01));

    let min_lat = latitude - lat_delta;
    let max_lat = latitude + lat_delta;
    let min_lon = longitude - lon_delta;
    let max_lon = longitude + lon_delta;

    // Fetch pages - configurable start and count
    let start_name = start_page_name.unwrap_or(1);
    let start_elements = start_page_elements.unwrap_or(1);
    let max_pages_this_request = pages_to_fetch.unwrap_or(30); // API returns ~10 per page
    let page_size = 100; // API ignores this and returns ~10 per page anyway

    // Helper to fetch localities with given filters
    async fn fetch_localities(
        client: &MindatClient,
        country: &Option<String>,
        name: Option<&String>,
        elements: Option<&String>,
        start_page: i32,
        max_pages: i32,
        page_size: i32,
    ) -> Result<(Vec<mindat_rs::Locality>, bool, i32), mindat_rs::MindatError> {
        let mut all_results = Vec::new();
        let mut current_page = start_page;
        let mut pages_fetched = 0;
        let mut has_more = false;
        let mut last_page = start_page;

        loop {
            let mut query = LocalitiesQuery::new().page(current_page).page_size(page_size);

            if let Some(ref c) = country {
                if !c.is_empty() {
                    query = query.country(c);
                }
            }
            if let Some(n) = name {
                if !n.is_empty() {
                    query = query.name_contains(n);
                }
            }
            if let Some(e) = elements {
                if !e.is_empty() {
                    query = query.with_elements(e);
                }
            }

            let response = client.localities(query).await?;
            let next_page = response.next_page();
            all_results.extend(response.results);
            last_page = current_page;
            pages_fetched += 1;

            if let Some(page) = next_page {
                if pages_fetched >= max_pages {
                    has_more = true;
                    break;
                }
                current_page = page;
            } else {
                break;
            }
        }

        Ok((all_results, has_more, last_page))
    }

    let mut all_results = Vec::new();
    let mut has_more_name = false;
    let mut has_more_elements = false;
    let mut next_page_name = 0;
    let mut next_page_elements = 0;

    // Determine search strategy
    let has_name = name_contains.as_ref().map(|s| !s.is_empty()).unwrap_or(false);
    let has_elements = elements_filter.as_ref().map(|s| !s.is_empty()).unwrap_or(false);

    if has_name && has_elements {
        // Run BOTH searches separately and merge (OR logic)
        let pages_each = max_pages_this_request / 2;

        // Search by name (only if we haven't exhausted name pages)
        if start_name > 0 {
            let (name_results, name_more, name_last) = fetch_localities(
                &client, &country, name_contains.as_ref(), None, start_name, pages_each, page_size
            ).await?;
            all_results.extend(name_results);
            has_more_name = name_more;
            next_page_name = if name_more { name_last + 1 } else { 0 };
        }

        // Search by elements (only if we haven't exhausted element pages)
        if start_elements > 0 {
            let (elem_results, elem_more, elem_last) = fetch_localities(
                &client, &country, None, elements_filter.as_ref(), start_elements, pages_each, page_size
            ).await?;
            all_results.extend(elem_results);
            has_more_elements = elem_more;
            next_page_elements = if elem_more { elem_last + 1 } else { 0 };
        }
    } else if has_name {
        // Search by name only
        let (results, more, last) = fetch_localities(
            &client, &country, name_contains.as_ref(), None, start_name, max_pages_this_request, page_size
        ).await?;
        all_results = results;
        has_more_name = more;
        next_page_name = if more { last + 1 } else { 0 };
    } else if has_elements {
        // Search by elements only
        let (results, more, last) = fetch_localities(
            &client, &country, None, elements_filter.as_ref(), start_elements, max_pages_this_request, page_size
        ).await?;
        all_results = results;
        has_more_elements = more;
        next_page_elements = if more { last + 1 } else { 0 };
    } else {
        // No filters - just country
        let (results, more, last) = fetch_localities(
            &client, &country, None, None, start_name, max_pages_this_request, page_size
        ).await?;
        all_results = results;
        has_more_name = more;  // Use name slot for generic search
        next_page_name = if more { last + 1 } else { 0 };
    }

    // Deduplicate by ID
    let mut seen_ids = std::collections::HashSet::new();
    all_results.retain(|loc| seen_ids.insert(loc.id));

    let before_filter = all_results.len();

    // Filter by bounding box
    let filtered: Vec<_> = all_results
        .into_iter()
        .filter(|loc| {
            if let (Some(lat), Some(lon)) = (loc.latitude, loc.longitude) {
                lat >= min_lat && lat <= max_lat && lon >= min_lon && lon <= max_lon
            } else {
                false
            }
        })
        .collect();

    #[cfg(debug_assertions)]
    eprintln!(
        "Search: before_filter={}, after_filter={}, bbox=[{:.2},{:.2}]-[{:.2},{:.2}], hasMore=name:{}/elem:{}",
        before_filter, filtered.len(), min_lat, min_lon, max_lat, max_lon,
        has_more_name, has_more_elements
    );

    let result = serde_json::json!({
        "results": filtered,
        "hasMore": has_more_name || has_more_elements,
        "nextPageName": next_page_name,
        "nextPageElements": next_page_elements,
        "count": filtered.len(),
        "beforeFilter": before_filter
    });

    Ok(result)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_geolocation::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            set_api_key,
            is_configured,
            search_localities_nearby,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
