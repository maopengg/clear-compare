//! Full-image statistics with aligned thumbnails, overlay and difference visualization.
use crate::tr;
use clear_contracts::PicturePreview;
use leptos::prelude::*;
#[derive(Clone, Copy)]
pub struct Signals {
    pub picture: RwSignal<Option<PicturePreview>>,
    pub picture_id: RwSignal<String>,
    pub picture_tolerance: RwSignal<String>,
    pub picture_ignore_alpha: RwSignal<bool>,
}
pub fn panel(
    english: RwSignal<bool>,
    busy: RwSignal<bool>,
    signals: Signals,
    load: impl Fn(String) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let Signals {
        picture,
        picture_id,
        picture_tolerance,
        picture_ignore_alpha,
    } = signals;
    let mode = RwSignal::new(String::from("split"));
    let opacity = RwSignal::new(50u32);
    let zoom = RwSignal::new(100u32);
    let dimensions = move || {
        picture.with(|p| {
            p.as_ref()
                .map(|p| {
                    format!(
                        "width:{}px;height:{}px",
                        p.preview_width * zoom.get() / 100,
                        p.preview_height * zoom.get() / 100
                    )
                })
                .unwrap_or_default()
        })
    };
    view! {
        <Show when=move ||picture.with(Option::is_some)><section class="picture-panel"><header><strong>{move ||picture.with(|p|p.as_ref().map(|p|p.path.clone()))}</strong><button class="quiet" on:click=move |_|picture.set(None)>{move ||tr(english.get(),"关闭图片比较","Close picture comparison")}</button></header>
            <div class="picture-controls"><label>{move ||tr(english.get(),"通道容差","Channel tolerance")}<input type="number" min="0" max="255" disabled=move ||busy.get() prop:value=move ||picture_tolerance.get() on:input=move |ev|picture_tolerance.set(event_target_value(&ev))/></label><label><input type="checkbox" disabled=move ||busy.get() prop:checked=move ||picture_ignore_alpha.get() on:change=move |ev|picture_ignore_alpha.set(event_target_checked(&ev))/>{move ||tr(english.get(),"忽略透明度通道","Ignore alpha channel")}</label><button class="quiet" disabled=move ||busy.get() on:click=move |_|load(picture_id.get_untracked())>{move ||tr(english.get(),"应用规则","Apply rules")}</button>
                <select aria-label="Picture view mode" prop:value=move ||mode.get() on:change=move |ev|mode.set(event_target_value(&ev))><option value="split">{move ||tr(english.get(),"并排","Side by side")}</option><option value="overlay">{move ||tr(english.get(),"叠加","Overlay")}</option><option value="difference">{move ||tr(english.get(),"差异图","Difference map")}</option></select>
                <label>{move ||tr(english.get(),"缩放","Zoom")}<input type="range" min="25" max="400" step="25" prop:value=move ||zoom.get() on:input=move |ev|zoom.set(event_target_value(&ev).parse::<u32>().unwrap_or(100).clamp(25,400))/><span>{move ||format!("{}%",zoom.get())}</span></label>
            </div>
            <p>{move ||picture.with(|p|p.as_ref().map(|p|{let dimensions=|d:Option<(u32,u32)>|d.map(|(w,h)|format!("{w}×{h}")).unwrap_or_else(||tr(english.get(),"不存在","Missing").into());let bounds=p.summary.bounds.as_ref().map(|b|format!("({}, {}) {}×{} px",b.x,b.y,b.width,b.height)).unwrap_or_else(||tr(english.get(),"无差异区域","No changed region").into());format!("{} {} · {} {} · {} / {} {} · {} {} · {}",tr(english.get(),"左侧","Left"),dimensions(p.summary.left_dimensions),tr(english.get(),"右侧","Right"),dimensions(p.summary.right_dimensions),p.summary.changed_pixels,p.summary.total_pixels,tr(english.get(),"像素不同","pixels changed"),tr(english.get(),"最大通道差","Maximum channel delta"),p.summary.max_channel_delta,bounds)}))}</p>
            <p>{move ||tr(english.get(),"按左上角对齐，统计使用完整 RGBA8 像素；预览最长边最多 1024，红色区块表示其中存在差异。","Top-left alignment, full RGBA8 pixel statistics; previews have a maximum 1024-pixel side. Red buckets contain changed pixels.")}</p>
            <Show when=move ||mode.get()=="split"><div class="picture-pair"><figure><figcaption>{move ||tr(english.get(),"左侧","Left")}</figcaption><div class="picture-scroll"><Show when=move ||picture.with(|p|p.as_ref().is_some_and(|p|p.left_png.is_some())) fallback=move ||view!{<div class="missing-picture" style=dimensions>{move ||tr(english.get(),"此侧没有图像","No image on this side")}</div>}><img alt="Left picture preview" style=dimensions src=move ||picture.with(|p|p.as_ref().and_then(|p|p.left_png.clone()).unwrap_or_default())/></Show></div></figure><figure><figcaption>{move ||tr(english.get(),"右侧","Right")}</figcaption><div class="picture-scroll"><Show when=move ||picture.with(|p|p.as_ref().is_some_and(|p|p.right_png.is_some())) fallback=move ||view!{<div class="missing-picture" style=dimensions>{move ||tr(english.get(),"此侧没有图像","No image on this side")}</div>}><img alt="Right picture preview" style=dimensions src=move ||picture.with(|p|p.as_ref().and_then(|p|p.right_png.clone()).unwrap_or_default())/></Show></div></figure></div></Show>
            <Show when=move ||mode.get()=="overlay"><label class="overlay-opacity">{move ||tr(english.get(),"右侧不透明度","Right opacity")}<input type="range" min="0" max="100" prop:value=move ||opacity.get() on:input=move |ev|opacity.set(event_target_value(&ev).parse::<u32>().unwrap_or(50).clamp(0,100))/><span>{move ||format!("{}%",opacity.get())}</span></label><div class="picture-scroll"><div class="picture-overlay" style=dimensions><Show when=move ||picture.with(|p|p.as_ref().is_some_and(|p|p.left_png.is_some()))><img alt="Left overlay base" src=move ||picture.with(|p|p.as_ref().and_then(|p|p.left_png.clone()).unwrap_or_default())/></Show><Show when=move ||picture.with(|p|p.as_ref().is_some_and(|p|p.right_png.is_some()))><img alt="Right overlay layer" style=move ||format!("opacity:{}",f64::from(opacity.get())/100.) src=move ||picture.with(|p|p.as_ref().and_then(|p|p.right_png.clone()).unwrap_or_default())/></Show></div></div></Show>
            <Show when=move ||mode.get()=="difference"><div class="picture-scroll"><img alt="Changed pixel map" style=dimensions src=move ||picture.with(|p|p.as_ref().map(|p|p.difference_png.clone()).unwrap_or_default())/></div></Show>
        </section></Show>
    }
}
