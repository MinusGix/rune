use rune::testing::*;

#[test]
fn test_nested_closures() {
    assert_eq! {
        4,
        rune! { i64 =>
            pub fn main() {
                let var = 1;

                let a = |i| {
                    let b = |j| {
                        var + j
                    };

                    b(i + 1)
                };

                a(2)
            }
        }
    };
}

#[test]
fn test_closure_in_loop_iter() {
    assert_eq! {
        10,
        rune! { i64 =>
            pub fn main() {
                let out = 1;

                for var in {
                    let a = || [1, 2, 3];
                    a()
                } {
                    let b = |n| var + n;
                    out += b(1);
                }

                out
            }
        }
    };
}

#[test]
fn test_capture_match() {
    assert_eq! {
        3,
        rune! { i64 =>
            pub fn main() {
                let n = 1;

                let a = match { let out = || Some(n + 1); out() } {
                    Some(n) => |e| n + e,
                    _ => |_| 0,
                };

                a(1)
            }
        }
    };
}

#[test]
fn test_capture_fn_arg() {
    assert_eq! {
        3,
        rune! { i64 =>
            fn foo(n) { |a| n + a }
            pub fn main() { foo(1)(2) }
        }
    };

    assert_eq! {
        4,
        rune! { i64 =>
            fn test(a, b) { b / a + 1 }
            pub fn main() {{let a = || test; a()}({let b = || 2; b()}, {let c = || 6; c()}) }
        }
    };

    assert_eq! {
        (2, 6),
        rune! { (i64, i64) =>
            pub fn main() { ({let b = || 2; b()}, {let c = || 6; c()}) }
        }
    };

    assert_eq! {
        vec![2, 6],
        rune! { Vec<i64> =>
            pub fn main() { [{let b = || 2; b()}, {let c = || 6; c()}] }
        }
    };
}

#[test]
fn test_capture_and_environ() {
    assert_eq! {
        13,
        rune! { i64 =>
            async fn foo(cb) {
                cb(1).await
            }

            pub async fn main() {
                let value = 12;
                foo(async |n| n + value).await
            }
        }
    };
}

#[test]
fn test_immediate_call() {
    assert_eq! {
        11,
        rune! { i64 =>
            pub async fn main() {
                let future = (async || {
                    11
                })();

                future.await
            }
        }
    };
}

#[test]
fn test_nested_async_closure() {
    assert_eq! {
        6,
        rune! { i64 =>
            async fn send_requests(list) {
                let input = 1;

                let do_request = async |url, n| {
                    Ok(input + n)
                };

                for url in list {
                    yield do_request(url, 2).await;
                }
            }

            pub async fn main() {
                let requests = send_requests([
                    "https://google.com",
                    "https://amazon.com",
                ]);

                let output = 0;

                while let Some(input) = requests.next().await {
                    output += input?;
                }

                output
            }
        }
    };
}

#[test]
fn test_closure_in_lit_vec() -> runestick::Result<()> {
    let ret = rune_s! {
        VecTuple<(i64, Function, Function, i64)> => r#"pub fn main() { let a = 4; [0, || 2, || 4, 3] }"#
    };

    let (start, first, second, end) = ret.0;
    assert_eq!(0, start);
    assert_eq!(2, first.call::<_, i64>(())?);
    assert_eq!(4, second.call::<_, i64>(())?);
    assert_eq!(3, end);
    Ok(())
}

#[test]
fn test_closure_in_lit_tuple() -> runestick::Result<()> {
    let ret = rune_s! {
        (i64, Function, Function, i64) => r#"pub fn main() { let a = 4; (0, || 2, || a, 3) }"#
    };

    let (start, first, second, end) = ret;
    assert_eq!(0, start);
    assert_eq!(2, first.call::<_, i64>(())?);
    assert_eq!(4, second.call::<_, i64>(())?);
    assert_eq!(3, end);
    Ok(())
}

#[test]
fn test_closure_in_lit_object() -> runestick::Result<()> {
    #[derive(FromValue)]
    struct Proxy {
        a: i64,
        b: Function,
        c: Function,
        d: i64,
    }

    let proxy = rune_s! {
        Proxy => r#"pub fn main() { let a = 4; #{a: 0, b: || 2, c: || a, d: 3} }"#
    };

    assert_eq!(0, proxy.a);
    assert_eq!(2, proxy.b.call::<_, i64>(())?);
    assert_eq!(4, proxy.c.call::<_, i64>(())?);
    assert_eq!(3, proxy.d);
    Ok(())
}
