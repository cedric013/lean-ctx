use axum::http::StatusCode;

use super::common::{ApiResult, err};
use super::payload::PublishPayload;
use crate::cloud_server::auth::AppState;
use crate::cloud_server::helpers::internal_error;

/// Loads a card and renders its SVG (shared by `card.svg` and `card.png`). 404 when unknown.
pub(in crate::cloud_server) async fn fetch_card_svg(
    state: &AppState,
    id: &str,
) -> ApiResult<String> {
    let client = state.pool.get().await.map_err(internal_error)?;
    let row = client
        .query_opt(
            "SELECT payload_json FROM wrapped_cards WHERE id = $1",
            &[&id],
        )
        .await
        .map_err(internal_error)?;
    let Some(row) = row else {
        return Err(err(StatusCode::NOT_FOUND, "not_found"));
    };
    let payload_json: String = row.get(0);
    let payload: PublishPayload = serde_json::from_str(&payload_json).map_err(internal_error)?;
    Ok(payload.to_report().to_svg())
}

/// Rasterizes an SVG string to PNG bytes via resvg. System fonts are loaded and a present
/// sans family is used as the fallback so headline text renders on headless servers.
pub(in crate::cloud_server) fn svg_to_png(svg: &str) -> Result<Vec<u8>, String> {
    use resvg::{tiny_skia, usvg};

    let mut opt = usvg::Options::default();
    // The card SVG declares web-font stacks (`Inter, …, sans-serif` and
    // `ui-monospace, …, monospace`) that don't exist on a headless server. usvg's default
    // generic families point at Windows fonts (Arial / Courier New), so on a slim image the
    // generic tail resolves to nothing and the headline renders blank. Map every generic
    // family onto DejaVu, which the container ships (`fonts-dejavu-core`), so all text
    // always rasterizes. (In usvg 0.47 the generic mappings live on the fontdb, not Options.)
    {
        let db = opt.fontdb_mut();
        db.load_system_fonts();
        db.set_serif_family("DejaVu Serif");
        db.set_sans_serif_family("DejaVu Sans");
        db.set_monospace_family("DejaVu Sans Mono");
    }
    opt.font_family = "DejaVu Sans".to_string();

    let tree = usvg::Tree::from_str(svg, &opt).map_err(|e| format!("svg parse: {e}"))?;
    let size = tree.size().to_int_size();
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| "pixmap alloc failed".to_string())?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| format!("png encode: {e}"))
}

/// Minimal HTML text escaping for the few user-derived strings on the permalink page.
pub(in crate::cloud_server) fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Renders the self-contained permalink page: per-card OG/Twitter meta + the inline card.
pub(in crate::cloud_server) fn render_permalink_html(
    id: &str,
    p: &PublishPayload,
    public_base: &str,
    api_base: &str,
) -> String {
    let report = p.to_report();
    let svg = report.to_svg();
    let tokens = crate::core::wrapped::format_tokens(report.tokens_saved);
    let cost = format!("${:.2}", report.cost_avoided_usd);
    let est = if report.pricing_estimated {
        " (est.)"
    } else {
        ""
    };

    let who = p.display_name.as_deref().map(html_escape);
    let title = match &who {
        Some(n) => format!("{n}'s lean-ctx Wrapped"),
        None => "lean-ctx Wrapped".to_string(),
    };
    let description = format!(
        "Saved {tokens} tokens (~{cost}{est}) with lean-ctx — my AI saw only what mattered."
    );

    let page_url = format!("{}/w/{}", public_base.trim_end_matches('/'), id);
    let img_url = format!(
        "{}/api/wrapped/{}/card.png",
        api_base.trim_end_matches('/'),
        id
    );

    let base = public_base.trim_end_matches('/');
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>{title}</title>
<meta name="description" content="{description}"/>
<link rel="canonical" href="{page_url}"/>
<meta property="og:type" content="website"/>
<meta property="og:site_name" content="lean-ctx"/>
<meta property="og:title" content="{title}"/>
<meta property="og:description" content="{description}"/>
<meta property="og:url" content="{page_url}"/>
<meta property="og:image" content="{img_url}"/>
<meta property="og:image:width" content="1200"/>
<meta property="og:image:height" content="630"/>
<meta name="twitter:card" content="summary_large_image"/>
<meta name="twitter:title" content="{title}"/>
<meta name="twitter:description" content="{description}"/>
<meta name="twitter:image" content="{img_url}"/>
{fonts}
<style>{css}</style>
</head>
<body>
{header}
<main class="lc-container">
<section class="lc-card-wrap">
{svg}
</section>
<section class="lc-cta-section">
<h2>Make your own Wrapped</h2>
<p>Install lean-ctx — your AI sees only what matters.</p>
<a class="lc-cta" href="{base}/docs/getting-started/">Install lean-ctx</a>
</section>
</main>
{footer}
</body>
</html>"#,
        fonts = crate::cloud_server::site_theme::FONT_LINKS,
        css = crate::cloud_server::site_theme::THEME_CSS,
        header = crate::cloud_server::site_theme::header(base),
        footer = crate::cloud_server::site_theme::footer(base),
    )
}
