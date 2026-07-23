use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};

use super::common::ApiResult;
use super::payload::PublishPayload;
use super::render::html_escape;
use crate::cloud_server::auth::AppState;
use crate::cloud_server::helpers::internal_error;

/// One row of the public leaderboard.
#[derive(Serialize)]
pub(in crate::cloud_server) struct LeaderRow {
    pub(in crate::cloud_server) rank: usize,
    pub(in crate::cloud_server) id: String,
    pub(in crate::cloud_server) url: String,
    pub(in crate::cloud_server) display_name: Option<String>,
    pub(in crate::cloud_server) tokens_saved: i64,
    pub(in crate::cloud_server) cost_avoided_usd: f64,
    pub(in crate::cloud_server) compression_rate_pct: f64,
    pub(in crate::cloud_server) period: String,
    pub(in crate::cloud_server) pricing_estimated: bool,
    /// Self-reported figures that look statistically implausible (very high compression over very
    /// large volume). Such cards are de-emphasized and badged rather than removed.
    pub(in crate::cloud_server) flagged: bool,
}

#[derive(Serialize)]
pub(in crate::cloud_server) struct Leaderboard {
    /// The requested page of ranked entries (global 1-based ranks preserved).
    pub(in crate::cloud_server) entries: Vec<LeaderRow>,
    /// 1-based page index actually returned (clamped to `1..=total_pages`).
    pub(in crate::cloud_server) page: i64,
    /// Entries per page actually applied (clamped to `1..=LEADERBOARD_PER_PAGE_MAX`).
    pub(in crate::cloud_server) per_page: i64,
    /// Size of the result set (after the optional `q` filter) — basis for `total_pages`.
    pub(in crate::cloud_server) total_entries: i64,
    /// Number of pages over `total_entries` at `per_page` (always >= 1).
    pub(in crate::cloud_server) total_pages: i64,
    /// Sum of `tokens_saved` across **all** opted-in accounts — uncapped and
    /// independent of `page`/`q`, so callers can show the true community total
    /// without walking every page. Grouping-invariant (#488): summing per-account
    /// equals summing per-machine, so the value is unaffected by stacking.
    pub(in crate::cloud_server) total_tokens_saved: i64,
    /// Sum of `cost_avoided_usd` across **all** opted-in accounts — the USD
    /// counterpart to `total_tokens_saved` (same uncapped, page-independent basis).
    pub(in crate::cloud_server) total_cost_avoided_usd: f64,
}

/// Query for both the JSON API and the SSR page: 1-based `page`, `per_page`
/// (clamped), and an optional case-insensitive `q` substring over `display_name`.
/// All fields optional so a bare `/api/leaderboard` stays back-compatible.
#[derive(Debug, Default, Deserialize)]
pub(in crate::cloud_server) struct LeaderboardQuery {
    pub(in crate::cloud_server) page: Option<i64>,
    pub(in crate::cloud_server) per_page: Option<i64>,
    pub(in crate::cloud_server) q: Option<String>,
}

/// Page size when the caller does not specify `per_page`.
const LEADERBOARD_PER_PAGE_DEFAULT: i64 = 50;
/// Upper bound on `per_page` so a single request can't pull an unbounded page.
pub(in crate::cloud_server) const LEADERBOARD_PER_PAGE_MAX: i64 = 100;

/// Hard safety guardrail on how many per-machine rows we pull before account
/// aggregation. The whole opted-in set is aggregated, ranked and paginated in
/// Rust — account stacking (#488) sums a user's machines, so we must see all of a
/// user's machines (a pre-aggregation top-N could undercount an account). Set far
/// above the current opted-in population so there is no visible cap; revisit with
/// SQL-side keyset pagination (`GROUP BY` + a Postgres harness) only if the board
/// ever approaches this many distinct machines.
const LEADERBOARD_MAX_CARDS: i64 = 50_000;

/// Compression rate (percent) at/above which a card is treated as implausible *when paired with
/// high volume*. Organic agent usage compresses reads by ~60-90% on average; a sustained rate
/// this high indicates cache-hit/automation-dominated or fabricated figures, not representative
/// savings. (See `IMPLAUSIBLE_MIN_TOKENS` — a high rate over a tiny sample is normal.)
pub(in crate::cloud_server) const IMPLAUSIBLE_RATE_PCT: f64 = 97.0;

/// Saved-token volume above which an extreme `IMPLAUSIBLE_RATE_PCT` is treated as implausible.
/// A near-100% rate over a few thousand tokens is an ordinary small-sample artefact; the same
/// rate sustained across this many tokens is not achievable by real coding work.
pub(in crate::cloud_server) const IMPLAUSIBLE_MIN_TOKENS: i64 = 1_000_000_000;

/// Leaderboard figures are **self-reported** from each publisher's local ledger — the server
/// holds no denominator (`tokens_input` is never uploaded; see `PublishPayload`) and therefore
/// cannot recompute the rate. This pure check flags cards whose figures are statistically
/// implausible so the board can de-emphasize and badge them instead of letting a single
/// unverifiable card top the ranking. Pure (no I/O) so it is unit-tested.
pub(in crate::cloud_server) fn stats_implausible(
    tokens_saved: i64,
    compression_rate_pct: f64,
) -> bool {
    tokens_saved >= IMPLAUSIBLE_MIN_TOKENS && compression_rate_pct >= IMPLAUSIBLE_RATE_PCT
}

/// Orders leaderboard rows for display: plausible cards first (preserving the incoming
/// `tokens_saved DESC` order from the query), flagged/implausible cards last, then assigns
/// 1-based ranks. `slice::sort_by_key` is stable, so each group keeps its relative order. Pure,
/// so the ordering rule is unit-tested without a database.
pub(in crate::cloud_server) fn rank_and_demote_flagged(entries: &mut [LeaderRow]) {
    entries.sort_by_key(|e| e.flagged);
    for (i, e) in entries.iter_mut().enumerate() {
        e.rank = i + 1;
    }
}

/// `GET /api/wrapped/leaderboard` — paginated opted-in cards by tokens saved. Public; the only
/// person-facing field is the user-chosen `display_name`. Supports `?page=&per_page=&q=`.
pub(in crate::cloud_server) async fn leaderboard(
    State(state): State<AppState>,
    Query(query): Query<LeaderboardQuery>,
) -> ApiResult<Json<Leaderboard>> {
    Ok(Json(build_leaderboard(&state, &query).await?))
}

/// `GET /leaderboard` — server-rendered leaderboard page (static hosts proxy `/leaderboard` here).
/// Honors the same `?page=&per_page=&q=` query as the JSON API.
pub(in crate::cloud_server) async fn get_leaderboard_page(
    State(state): State<AppState>,
    Query(query): Query<LeaderboardQuery>,
) -> ApiResult<axum::response::Response> {
    let board = build_leaderboard(&state, &query).await?;
    let html = render_leaderboard_html(&board, query.q.as_deref(), &state.cfg.public_base_url);

    use axum::http::header::CONTENT_TYPE;
    use axum::response::IntoResponse;
    Ok(([(CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response())
}

/// Fetch → aggregate → rank the full opted-in board, then apply the optional `q`
/// filter and slice the requested page.
pub(in crate::cloud_server) async fn build_leaderboard(
    state: &AppState,
    query: &LeaderboardQuery,
) -> ApiResult<Leaderboard> {
    let all = all_ranked_cards(state).await?;
    Ok(paginate(all, query))
}

/// Slice a fully-ranked board into one page. Pure (no I/O) so the pagination,
/// clamping and `q`-filter rules are unit-tested without a database.
pub(in crate::cloud_server) fn paginate(
    all: Vec<LeaderRow>,
    query: &LeaderboardQuery,
) -> Leaderboard {
    // Community totals are grouping-invariant and must ignore both page and
    // filter, so they are summed over the full board before anything is dropped.
    let total_tokens_saved = all
        .iter()
        .map(|e| e.tokens_saved)
        .fold(0i64, i64::saturating_add);
    let total_cost_avoided_usd = all.iter().map(|e| e.cost_avoided_usd).sum();

    // Optional case-insensitive substring on the display name. Ranks were assigned
    // over the full board, so a matched user keeps their real (global) rank.
    let filtered: Vec<LeaderRow> = match query.q.as_deref().map(str::trim) {
        Some(needle) if !needle.is_empty() => {
            let needle = needle.to_lowercase();
            all.into_iter()
                .filter(|e| {
                    e.display_name
                        .as_deref()
                        .is_some_and(|n| n.to_lowercase().contains(&needle))
                })
                .collect()
        }
        _ => all,
    };

    let per_page = query
        .per_page
        .unwrap_or(LEADERBOARD_PER_PAGE_DEFAULT)
        .clamp(1, LEADERBOARD_PER_PAGE_MAX);
    let total_entries = filtered.len() as i64;
    // Ceiling division, computed by hand: `i64::div_ceil` is still unstable on our
    // toolchain. `per_page >= 1`, so this never divides by zero; an empty board
    // still reports a single (empty) page.
    let total_pages = ((total_entries + per_page - 1) / per_page).max(1);
    let page = query.page.unwrap_or(1).clamp(1, total_pages);

    let start = ((page - 1) * per_page) as usize;
    let entries: Vec<LeaderRow> = filtered
        .into_iter()
        .skip(start)
        .take(per_page as usize)
        .collect();

    Leaderboard {
        entries,
        page,
        per_page,
        total_entries,
        total_pages,
        total_tokens_saved,
        total_cost_avoided_usd,
    }
}

/// The entire opted-in board: one representative row per machine, account-stacked
/// (#488), then ranked with flagged cards demoted. Ranks are global (1-based over
/// the whole board) so pagination shows true ranks (page 2 starts at `per_page+1`).
pub(in crate::cloud_server) async fn all_ranked_cards(
    state: &AppState,
) -> ApiResult<Vec<LeaderRow>> {
    let client = state.pool.get().await.map_err(internal_error)?;
    // One representative row per machine (its highest-saving card); legacy anonymous rows
    // (publisher_id NULL) stay distinct via COALESCE(publisher_id, id). `user_id` is carried
    // through so machines claimed to the same account can be stacked below (#488).
    let rows = client
        .query(
            "SELECT id, payload_json, user_id::text, link_group FROM ( \
               SELECT DISTINCT ON (COALESCE(publisher_id, id)) \
                      id, payload_json, tokens_saved, created_at, user_id, link_group \
               FROM wrapped_cards \
               WHERE leaderboard_opt_in = TRUE \
               ORDER BY COALESCE(publisher_id, id), tokens_saved DESC, created_at DESC \
             ) t \
             ORDER BY tokens_saved DESC, created_at DESC LIMIT $1",
            &[&LEADERBOARD_MAX_CARDS],
        )
        .await
        .map_err(internal_error)?;

    let raw: Vec<RawLeaderCard> = rows
        .iter()
        .map(|r| RawLeaderCard {
            id: r.get(0),
            payload_json: r.get(1),
            user_id: r.get(2),
            link_group: r.get(3),
        })
        .collect();

    let base = state.cfg.public_base_url.trim_end_matches('/');
    let mut entries = aggregate_by_account(raw, base);

    // Plausible cards rank first; flagged (implausible, unverifiable) cards sink to the bottom
    // regardless of raw `tokens_saved`, so one unverifiable card can't top the board.
    rank_and_demote_flagged(&mut entries);
    Ok(entries)
}

/// A per-machine leaderboard row as fetched from the DB, before account aggregation.
/// The SQL guarantees one row per machine (`DISTINCT ON (COALESCE(publisher_id, id))`),
/// so the aggregation only needs the two merge dimensions, not the machine key itself.
pub(in crate::cloud_server) struct RawLeaderCard {
    pub(in crate::cloud_server) id: String,
    pub(in crate::cloud_server) payload_json: String,
    /// Account id (`user_id`) as text — present once the card is claimed (`claim_card`).
    pub(in crate::cloud_server) user_id: Option<String>,
    /// Login-less pairing group (GH #736) — present once linked via `link_complete`.
    pub(in crate::cloud_server) link_group: Option<String>,
}

/// Collapse a user's machines into one leaderboard entry (#488, #736).
///
/// Reported by a user: publishing from two machines produced two leaderboard rows.
/// The machine identity (`publisher_id`) is derived per device, so each machine is
/// distinct by construction. Two ways to merge machines exist, and they compose:
///
/// - **Account claim** (#488): cards sharing a `user_id` stack (login-based).
/// - **Login-less pairing** (#736): cards sharing a `link_group` stack
///   (`gain --link`, edit_token-authorized, no account).
///
/// Grouping is *transitive across both dimensions* (union-find): if card A and
/// B share a `link_group` and B and C share a `user_id`, all three form one
/// entry. Merged entries sum `tokens_saved` / `cost_avoided_usd`, the
/// highest-saving machine is the representative (display name + card URL), and
/// the compression rate is token-weighted. Cards with neither dimension stay
/// individual, keyed by `pub_key`. Pure (no I/O) so the stacking rule is
/// unit-tested without a database.
pub(in crate::cloud_server) fn aggregate_by_account(
    raw: Vec<RawLeaderCard>,
    base: &str,
) -> Vec<LeaderRow> {
    use std::collections::HashMap;

    struct Acc {
        rep_tokens: i64,
        rep_id: String,
        rep_display_name: Option<String>,
        rep_period: String,
        rep_rate: f64,
        sum_tokens: i64,
        sum_cost: f64,
        rate_num: f64,
        rate_den: i64,
        pricing_estimated: bool,
    }

    // Union-find over card indices: cards sharing a user_id OR a link_group
    // collapse into one cluster (transitively).
    fn find(parent: &mut Vec<usize>, i: usize) -> usize {
        if parent[i] != i {
            let root = find(parent, parent[i]);
            parent[i] = root;
        }
        parent[i]
    }
    fn union(parent: &mut Vec<usize>, a: usize, b: usize) {
        let (ra, rb) = (find(parent, a), find(parent, b));
        if ra != rb {
            parent[rb] = ra;
        }
    }

    // Every card starts as its own cluster (the SQL already yields one row per
    // machine, so the row index is a stable identity here); shared user_ids
    // and shared link_groups merge clusters.
    let mut parent: Vec<usize> = (0..raw.len()).collect();
    let mut by_user: HashMap<&str, usize> = HashMap::new();
    let mut by_group: HashMap<&str, usize> = HashMap::new();
    for (i, c) in raw.iter().enumerate() {
        if let Some(u) = c.user_id.as_deref().filter(|u| !u.is_empty()) {
            match by_user.get(u) {
                Some(&j) => union(&mut parent, j, i),
                None => {
                    by_user.insert(u, i);
                }
            }
        }
        if let Some(g) = c.link_group.as_deref().filter(|g| !g.is_empty()) {
            match by_group.get(g) {
                Some(&j) => union(&mut parent, j, i),
                None => {
                    by_group.insert(g, i);
                }
            }
        }
    }
    let cluster_of: Vec<usize> = (0..raw.len()).map(|i| find(&mut parent, i)).collect();

    let mut groups: HashMap<usize, Acc> = HashMap::new();
    for (i, c) in raw.into_iter().enumerate() {
        let Ok(p) = serde_json::from_str::<PublishPayload>(&c.payload_json) else {
            continue;
        };
        let acc = groups.entry(cluster_of[i]).or_insert_with(|| Acc {
            rep_tokens: i64::MIN,
            rep_id: String::new(),
            rep_display_name: None,
            rep_period: String::new(),
            rep_rate: 0.0,
            sum_tokens: 0,
            sum_cost: 0.0,
            rate_num: 0.0,
            rate_den: 0,
            pricing_estimated: false,
        });

        let tokens = p.tokens_saved;
        acc.sum_tokens = acc.sum_tokens.saturating_add(tokens);
        acc.sum_cost += p.cost_avoided_usd;
        if tokens > 0 {
            acc.rate_num += p.compression_rate_pct * tokens as f64;
            acc.rate_den = acc.rate_den.saturating_add(tokens);
        }
        acc.pricing_estimated |= p.pricing_estimated;
        // The highest-saving machine represents the account (display name + card URL).
        if tokens > acc.rep_tokens {
            acc.rep_tokens = tokens;
            acc.rep_id = c.id;
            acc.rep_display_name = p.display_name;
            acc.rep_period = p.period;
            acc.rep_rate = p.compression_rate_pct;
        }
    }

    let mut rows: Vec<LeaderRow> = groups
        .into_values()
        .map(|a| {
            // Token-weighted average rate across the account's machines (a plain mean would let a
            // tiny high-rate machine distort the figure); fall back to the representative's rate
            // when there is no positive volume to weight by.
            let rate = if a.rate_den > 0 {
                a.rate_num / a.rate_den as f64
            } else {
                a.rep_rate
            };
            LeaderRow {
                rank: 0, // assigned after reordering by the caller
                url: format!("{base}/w/{}", a.rep_id),
                id: a.rep_id,
                display_name: a.rep_display_name,
                tokens_saved: a.sum_tokens,
                cost_avoided_usd: a.sum_cost,
                compression_rate_pct: rate,
                period: a.rep_period,
                pricing_estimated: a.pricing_estimated,
                flagged: stats_implausible(a.sum_tokens, rate),
            }
        })
        .collect();

    // Deterministic order independent of HashMap iteration: highest stacked savings first,
    // ties broken by the representative card id.
    rows.sort_by(|x, y| {
        y.tokens_saved
            .cmp(&x.tokens_saved)
            .then_with(|| x.id.cmp(&y.id))
    });
    rows
}

pub(in crate::cloud_server) fn render_leaderboard_html(
    board: &Leaderboard,
    q: Option<&str>,
    public_base: &str,
) -> String {
    let base = public_base.trim_end_matches('/');
    let needle = q.map(str::trim).filter(|s| !s.is_empty());

    let mut items = String::new();
    for row in &board.entries {
        let name = row
            .display_name
            .as_deref()
            .map_or_else(|| "anonymous".to_string(), html_escape);
        let tokens_u = u64::try_from(row.tokens_saved).unwrap_or(0);
        let tokens = crate::core::wrapped::format_tokens(tokens_u);
        let energy = crate::core::energy::format_for_tokens(tokens_u);
        let comp = format!("{:.0}%", row.compression_rate_pct);
        let est = if row.pricing_estimated { " est." } else { "" };
        // Flagged cards never get the top-rank highlight; they carry an "unverified" badge instead.
        let rank_class = if row.flagged {
            " lc-flagged"
        } else {
            match row.rank {
                1 => " lc-rank-1",
                2 => " lc-rank-2",
                3 => " lc-rank-3",
                _ => "",
            }
        };
        let flag_badge = if row.flagged {
            r#"<span class="lc-flag" title="Self-reported figures that look statistically implausible (very high compression over very large volume). Not server-verified.">unverified</span>"#
        } else {
            ""
        };
        items.push_str(&format!(
            r#"<li><a class="lc-row{rank_class}" href="{url}"><span class="lc-rank">#{rank}</span><span class="lc-id"><span class="lc-name">{name}</span><span class="lc-period">{period}</span>{flag_badge}</span><span class="lc-stats"><span class="lc-num">{tokens}</span><span class="lc-meta">{comp} compressed · {energy} saved</span><span class="lc-usd">${cost:.0}{est}</span></span></a></li>"#,
            url = row.url,
            rank = row.rank,
            cost = row.cost_avoided_usd,
            period = html_escape(&row.period),
        ));
    }

    let board_html = if board.entries.is_empty() {
        match needle {
            Some(term) => format!(
                r#"<div class="lc-empty">No one on the board matches <strong>{term}</strong>.<br/><a href="{base}/leaderboard">Clear search</a></div>"#,
                term = html_escape(term)
            ),
            None => r#"<div class="lc-empty">No one has opted in yet — be the first:<br/><code>lean-ctx gain --publish --leaderboard</code></div>"#.to_string(),
        }
    } else {
        format!(r#"<ol class="lc-board">{items}</ol>"#)
    };

    // Search box — a GET to /leaderboard, so submitting resets to page 1 at the
    // default page size. Makes any entry findable by name, not just by paging.
    let q_attr = needle.map(html_escape).unwrap_or_default();
    let clear = if needle.is_some() {
        format!(r#"<a class="lc-search-clear" href="{base}/leaderboard">Clear</a>"#)
    } else {
        String::new()
    };
    let search = format!(
        r#"<form class="lc-search" method="get" action="{base}/leaderboard" role="search">
<input class="lc-search-input" type="search" name="q" value="{q_attr}" placeholder="Search by name…" aria-label="Search the leaderboard by name" autocomplete="off"/>
<button class="lc-search-btn" type="submit">Search</button>
{clear}
</form>"#
    );

    // Count line doubles as the "no cap" signal: it states the full result size.
    let count_line = if board.total_entries > 0 {
        let n = board.total_entries;
        let label = if n == 1 { "entry" } else { "entries" };
        match needle {
            Some(term) => format!(
                r#"<p class="lc-count">{n} {label} matching <strong>{term}</strong></p>"#,
                term = html_escape(term)
            ),
            None => format!(r#"<p class="lc-count">{n} {label} on the board</p>"#),
        }
    } else {
        String::new()
    };

    // Pagination links preserve the active search and any non-default page size.
    let q_param = needle
        .map(|t| format!("&q={}", urlencoding::encode(t)))
        .unwrap_or_default();
    let pp_param = if board.per_page == LEADERBOARD_PER_PAGE_DEFAULT {
        String::new()
    } else {
        format!("&per_page={}", board.per_page)
    };
    let page_url = |n: i64| format!("{base}/leaderboard?page={n}{pp_param}{q_param}");
    let pagination = if board.total_pages > 1 {
        let prev = if board.page > 1 {
            format!(
                r#"<a class="lc-page-btn" href="{}" rel="prev">← Prev</a>"#,
                page_url(board.page - 1)
            )
        } else {
            r#"<span class="lc-page-btn lc-page-btn-off" aria-disabled="true">← Prev</span>"#
                .to_string()
        };
        let next = if board.page < board.total_pages {
            format!(
                r#"<a class="lc-page-btn" href="{}" rel="next">Next →</a>"#,
                page_url(board.page + 1)
            )
        } else {
            r#"<span class="lc-page-btn lc-page-btn-off" aria-disabled="true">Next →</span>"#
                .to_string()
        };
        format!(
            r#"<nav class="lc-pagination" aria-label="Leaderboard pages">{prev}<span class="lc-page-info">Page {page} / {total}</span>{next}</nav>"#,
            page = board.page,
            total = board.total_pages,
        )
    } else {
        String::new()
    };

    let head = format!(
        r#"<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>lean-ctx Leaderboard — top realized token savings</title>
<meta name="description" content="Top token savings, opted in by lean-ctx users. Open source — your AI sees only what matters."/>
<link rel="canonical" href="{base}/leaderboard"/>
{fonts}
<style>{css}</style>"#,
        fonts = crate::cloud_server::site_theme::FONT_LINKS,
        css = crate::cloud_server::site_theme::THEME_CSS,
    );

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
{head}
</head>
<body>
{header}
<main class="lc-container">
<section class="lc-hero">
<span class="lc-label">Self-reported savings</span>
<h1>Leaderboard</h1>
<p>The most realized token savings, opted in by lean-ctx users. Figures are self-reported from each user's local ledger — not server-verified. Cards whose stats look statistically implausible are flagged <span class="lc-flag">unverified</span> and ranked last.</p>
</section>
{search}
{count_line}
{board_html}
{pagination}
<section class="lc-cta-section">
<h2>Put your savings on the board</h2>
<p>Install lean-ctx, then publish your Wrapped recap.</p>
<a class="lc-cta" href="{base}/docs/getting-started/">Install lean-ctx</a>
</section>
</main>
{footer}
</body>
</html>"#,
        header = crate::cloud_server::site_theme::header(base),
        footer = crate::cloud_server::site_theme::footer(base),
    )
}
