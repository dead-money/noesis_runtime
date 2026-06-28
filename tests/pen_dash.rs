//! `Pen::set_dash_style`: typed `&[f32]` dash pattern + offset round-trip through
//! the live `Noesis::DashStyle`, including the clear path (empty slice → solid stroke).

use noesis_runtime::brushes::SolidColorBrush;
use noesis_runtime::drawing::Pen;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-4
}

#[test]
fn pen_dash_style_roundtrip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let brush = SolidColorBrush::new([1.0, 0.0, 0.0, 1.0]);
        let mut pen = Pen::new(&brush, 2.0);

        assert!(pen.dashes().is_none(), "no dash style initially");
        assert!(pen.dash_offset().is_none(), "no dash offset initially");

        assert!(
            pen.set_dash_style(&[2.0, 1.0, 3.0], 0.5),
            "set_dash_style on a live Pen"
        );

        let dashes = pen.dashes().expect("dash pattern read back");
        assert_eq!(
            dashes,
            vec![2.0, 1.0, 3.0],
            "dash pattern survived the round-trip through Noesis::DashStyle"
        );
        assert!(
            approx(pen.dash_offset().expect("offset read back"), 0.5),
            "dash offset round-trip"
        );

        assert!(pen.set_dash_style(&[4.0, 4.0], 1.25));
        assert_eq!(
            pen.dashes().expect("dashes"),
            vec![4.0, 4.0],
            "pattern replaced"
        );
        assert!(
            approx(pen.dash_offset().expect("offset"), 1.25),
            "offset replaced"
        );

        assert!(pen.set_dash_style(&[], 0.0), "clear dash style");
        assert!(pen.dashes().is_none(), "dash style cleared");
        assert!(pen.dash_offset().is_none(), "dash offset cleared");
    }

    noesis_runtime::shutdown();
}
