//! Error, assert, and per-thread error handler round-trips.
//!
//! All invokes use `fatal = false`; a fatal error or real assert can abort the
//! process.
//!
//! A Release Noesis build compiles the assert subsystem out
//! (`SetAssertHandler`/`InvokeAssertHandler` are a no-op stub), so the assert
//! handler never runs there; the test accepts either build.

use std::sync::{Arc, Mutex};

use noesis_runtime::diagnostics::{self as diag, ErrorContext};

type Rec<T> = Arc<Mutex<Vec<T>>>;

fn rec<T>() -> Rec<T> {
    Arc::new(Mutex::new(Vec::new()))
}

#[test]
fn error_assert_thread_handlers() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let a: Rec<(String, u32, String, bool)> = rec();
        {
            let a2 = Arc::clone(&a);
            let _ga = diag::set_error_handler(move |file, line, msg, fatal| {
                a2.lock()
                    .unwrap()
                    .push((file.into(), line, msg.into(), fatal));
            });

            diag::invoke_error("widgets.cpp", 128, false, "thing went sideways");
            {
                let got = a.lock().unwrap();
                assert_eq!(got.len(), 1, "error handler fired exactly once");
                assert_eq!(
                    got[0],
                    (
                        "widgets.cpp".to_string(),
                        128,
                        "thing went sideways".to_string(),
                        false
                    ),
                    "file/line/message/fatal must round-trip verbatim across the FFI"
                );
            }

            let b: Rec<(String, u32, String)> = rec();
            {
                let b2 = Arc::clone(&b);
                let _gb = diag::set_error_handler(move |file, line, msg, _fatal| {
                    b2.lock().unwrap().push((file.into(), line, msg.into()));
                });
                diag::invoke_error("inner.cpp", 7, false, "inner only");
                assert_eq!(
                    b.lock().unwrap().len(),
                    1,
                    "inner handler caught its invoke"
                );
                assert_eq!(
                    a.lock().unwrap().len(),
                    1,
                    "outer handler must NOT see the inner invoke while shadowed"
                );
                // Single slot, last registration wins: dropping `_gb` restores
                // Noesis's default, not the replaced outer handler.
            }

            diag::invoke_error("outer.cpp", 9, false, "outer again");
            assert_eq!(
                a.lock().unwrap().len(),
                1,
                "a replaced handler is NOT restored when its successor drops"
            );
            assert_eq!(
                b.lock().unwrap().len(),
                1,
                "dropped inner handler must NOT receive later invokes"
            );
        }

        diag::invoke_error("after.cpp", 1, false, "to the default");
        assert_eq!(
            a.lock().unwrap().len(),
            1,
            "no invoke may reach a handler after it was replaced/dropped"
        );

        let t: Rec<(String, u32, String, bool, Option<ErrorContext>)> = rec();
        {
            let t2 = Arc::clone(&t);
            let _gt = diag::set_thread_error_handler(move |file, line, msg, fatal, ctx| {
                t2.lock()
                    .unwrap()
                    .push((file.into(), line, msg.into(), fatal, ctx.cloned()));
            });

            diag::invoke_error_with_context(
                "parser.cpp",
                55,
                false,
                "MainWindow.xaml",
                12,
                34,
                "unexpected token",
            );
            {
                let got = t.lock().unwrap();
                assert_eq!(got.len(), 1, "thread error handler fired once");
                let (file, line, msg, fatal, ctx) = &got[0];
                assert_eq!(file, "parser.cpp");
                assert_eq!(*line, 55);
                assert_eq!(msg, "unexpected token");
                assert!(!*fatal);
                assert_eq!(
                    ctx.as_ref(),
                    Some(&ErrorContext {
                        uri: Some("MainWindow.xaml".to_string()),
                        line: 12,
                        column: 34,
                    }),
                    "ErrorContext uri/line/column must round-trip through ErrorHandler2"
                );
            }

            diag::invoke_error("parser.cpp", 56, false, "no ctx here");
            {
                let got = t.lock().unwrap();
                assert_eq!(got.len(), 2);
                assert_eq!(got[1].4, None, "absent context must surface as None");
            }
        }

        diag::invoke_error_with_context("parser.cpp", 99, false, "x.xaml", 1, 1, "post-drop");
        assert_eq!(
            t.lock().unwrap().len(),
            2,
            "dropped thread handler must receive nothing further"
        );

        let s: Rec<(String, u32, String)> = rec();
        {
            let s2 = Arc::clone(&s);
            let _gs = diag::set_assert_handler(move |file, line, expr| {
                s2.lock().unwrap().push((file.into(), line, expr.into()));
                true // request a debug break
            });
            let ret = diag::invoke_assert("checks.cpp", 200, "ptr != nullptr");
            let recorded = s.lock().unwrap().clone();

            if recorded.is_empty() {
                // Release SDK: the stubbed invoker returns false.
                assert!(
                    !ret,
                    "stubbed InvokeAssertHandler must return false on a Release SDK"
                );
            } else {
                // Debug SDK: the handler ran and its `true` propagates.
                assert_eq!(recorded.len(), 1, "assert handler fired exactly once");
                assert_eq!(
                    recorded[0],
                    ("checks.cpp".to_string(), 200, "ptr != nullptr".to_string()),
                    "assert file/line/expr must round-trip"
                );
                assert!(
                    ret,
                    "closure returned true → InvokeAssertHandler must return true"
                );
            }
        }
    }

    noesis_runtime::shutdown();
}
