use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};
#[derive(Clone, Copy)]
pub(crate) enum Icon {
    Box,
    Fillet,
    Parameters,
    Tree,
    Plus,
    Minus,
    Fit,
    Undo,
    Redo,
    Refresh,
    Connect,
    Download,
}
pub(crate) fn tool(ui: &mut Ui, icon: Icon, label: &str, active: bool, enabled: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(52., 60.), Sense::click());
    let ink = if !enabled {
        Color32::from_gray(100)
    } else if active {
        Color32::from_rgb(44, 210, 190)
    } else {
        Color32::from_rgb(210, 222, 232)
    };
    if active || response.hovered() && enabled {
        ui.painter().rect_filled(
            rect.shrink(1.),
            6.,
            if active {
                Color32::from_rgb(22, 73, 74)
            } else {
                Color32::from_rgb(47, 59, 70)
            },
        );
    }
    draw(
        ui,
        icon,
        Rect::from_center_size(rect.center() - Vec2::new(0., 8.), Vec2::splat(24.)),
        ink,
    );
    ui.painter().text(
        rect.center() + Vec2::new(0., 19.),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(10.),
        ink,
    );
    response.on_hover_text(label)
}
fn draw(ui: &Ui, icon: Icon, r: Rect, c: Color32) {
    let p = ui.painter();
    let s = Stroke::new(1.8, c);
    let o = r.left_top();
    let at = |x, y| o + Vec2::new(x, y);
    let line = |a: Pos2, b: Pos2| {
        p.line_segment([a, b], s);
    };
    match icon {
        Icon::Box => {
            for (a, b) in [
                ([2., 7.], [12., 2.]),
                ([12., 2.], [22., 7.]),
                ([22., 7.], [12., 12.]),
                ([12., 12.], [2., 7.]),
                ([2., 7.], [2., 18.]),
                ([22., 7.], [22., 18.]),
                ([12., 12.], [12., 23.]),
                ([2., 18.], [12., 23.]),
                ([12., 23.], [22., 18.]),
            ] {
                line(at(a[0], a[1]), at(b[0], b[1]));
            }
        }
        Icon::Fillet => {
            line(at(3., 3.), at(3., 21.));
            line(at(3., 21.), at(22., 21.));
            let points = (0..=18)
                .map(|i| {
                    let a = i as f32 / 18. * std::f32::consts::FRAC_PI_2;
                    at(20. - 17. * a.cos(), 4. + 17. * a.sin())
                })
                .collect();
            p.add(egui::Shape::line(points, Stroke::new(2.8, c)));
        }
        Icon::Parameters => {
            for (y, x) in [(5., 8.), (12., 17.), (20., 11.)] {
                line(at(2., y), at(22., y));
                p.circle_filled(at(x, y), 3., c);
            }
        }
        Icon::Tree => {
            line(at(5., 3.), at(5., 20.));
            for y in [5., 12., 20.] {
                line(at(5., y), at(13., y));
                p.rect_stroke(
                    Rect::from_min_size(at(13., y - 2.5), Vec2::new(8., 5.)),
                    1.,
                    s,
                    egui::StrokeKind::Inside,
                );
            }
        }
        Icon::Plus | Icon::Minus => {
            p.circle_stroke(at(10., 10.), 7., s);
            line(at(15., 15.), at(23., 23.));
            line(at(6., 10.), at(14., 10.));
            if matches!(icon, Icon::Plus) {
                line(at(10., 6.), at(10., 14.));
            }
        }
        Icon::Fit => {
            for (x, y, sx, sy) in [
                (2., 2., 1., 1.),
                (22., 2., -1., 1.),
                (2., 22., 1., -1.),
                (22., 22., -1., -1.),
            ] {
                line(at(x, y), at(x + 6. * sx, y));
                line(at(x, y), at(x, y + 6. * sy));
            }
            p.rect_stroke(r.shrink(7.), 1., s, egui::StrokeKind::Inside);
        }
        Icon::Undo | Icon::Redo | Icon::Refresh => {
            let flip = matches!(icon, Icon::Redo);
            let q = |x, y| at(if flip { 24. - x } else { x }, y);
            let points = (0..=22)
                .map(|i| {
                    let a = -2.4 + i as f32 / 22. * 4.8;
                    q(12. + 9. * a.cos(), 13. + 8. * a.sin())
                })
                .collect();
            p.add(egui::Shape::line(points, s));
            line(q(3., 2.), q(4., 9.));
            line(q(4., 9.), q(11., 7.));
        }
        Icon::Connect => {
            p.rect_stroke(
                Rect::from_min_size(at(6., 7.), Vec2::new(12., 10.)),
                3.,
                s,
                egui::StrokeKind::Inside,
            );
            line(at(9., 2.), at(9., 7.));
            line(at(15., 2.), at(15., 7.));
            line(at(12., 17.), at(12., 23.));
        }
        Icon::Download => {
            line(at(12., 2.), at(12., 16.));
            line(at(7., 11.), at(12., 16.));
            line(at(12., 16.), at(17., 11.));
            line(at(3., 17.), at(3., 22.));
            line(at(3., 22.), at(21., 22.));
            line(at(21., 22.), at(21., 17.));
        }
    }
}
