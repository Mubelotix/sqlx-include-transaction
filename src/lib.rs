use proc_macro::{Ident, TokenStream, TokenTree, Span};

fn caseless_contains(text: &str, needle_lc: &str) -> bool {
    if text.len() < needle_lc.len() {
        return false;
    }

    let text = text.as_bytes();
    let needle_lc = needle_lc.as_bytes();
    'offset: for i in 0..text.len()-needle_lc.len() {
        for j in 0..needle_lc.len() {
            if text[i+j] != needle_lc[j] && (text[i+j] < 65 || text[i+j] > 90 || text[i+j] + 32 != needle_lc[j]) {
                continue 'offset;
            }
        }
        return true;
    }
    false
}

fn sql_to_code(sql: &str, bindings: &[String]) -> String {
    let mut code = String::new(); // TODO: Using a string is convenient, but it's not the fastest way to build a TokenStream
    code.push('{');
    code.push_str("let mut tx = pool.begin().await?;\n");

    let mut output_vars = 1;
    let mut input_vars = 1;
    for query in sql.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        match caseless_contains(query, "returning") {
            true => {
                code.push_str(&format!("let var{vars} = sqlx::query_as::<_, _>(\"{query};\")"));
                while query.contains(&format!("${}", input_vars)) { // TODO: investigate potential bug with double digit input vars
                    code.push_str(&format!(".bind({})", bindings[input_vars-1]));
                    input_vars += 1;
                }
                code.push_str(".fetch_one(&mut *tx).await?;\n");
                output_vars += 1;
            },
            false => {
                code.push_str(&format!("sqlx::query(\"{query};\")"));
                while query.contains(&format!("${}", input_vars)) {
                    code.push_str(&format!(".bind({})", bindings[input_vars-1]));
                    input_vars += 1;
                }
                code.push_str(".execute(&mut *tx).await?;\n");
            }
        }
    }
    code.push_str("tx.commit().await?;\n");
    code.push_str(&format!("({})", (0..output_vars-1).map(|i| format!("var{i}")).collect::<Vec<_>>().join(", ")));
    code.push('}');

    if input_vars-1 != bindings.len() {
        panic!("Expected {} input variables, but only {} were provided", input_vars, bindings.len());
    }

    code
}

//#[proc_macro_error]
fn include_tx_inner(input: TokenStream) -> String {
    let mut tokens = input.into_iter();
    let lit = match tokens.next() {
        Some(TokenTree::Literal(lit)) => lit,
        Some(_) => panic!("The filename argument must be a string literal"),
        None => panic!("Expected a filename argument")
    };

    let mut bindings = Vec::new();
    while let Some(token) = tokens.next() {
        match token {
            TokenTree::Punct(punct) if punct.as_char() == ',' => {},
            _ => panic!("Arguments must be separated by commas")
        }

        match tokens.next() {
            Some(TokenTree::Ident(ident)) => bindings.push(ident),
            Some(_) => panic!("Bindings must be identifiers"),
            None => break
        }
    }

    let filename = lit.to_string();
    if !filename.starts_with('"') || !filename.ends_with('"') {
        panic!("The filename argument must be a string literal")
    }
    let filename = &filename[1..filename.len()-1];

    let sql = std::fs::read_to_string(filename).unwrap_or_else(|_| {
        panic!("Failed to read file {}", filename)
    });

    let bindings = bindings.into_iter().map(|i| i.to_string()).collect::<Vec<_>>();
    sql_to_code(&sql, &bindings)
}

#[proc_macro]
pub fn include_tx(input: TokenStream) -> TokenStream {
    let code = include_tx_inner(input);
    code.parse().expect("Internal error: bad code generated")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_caseless_contains() {
        assert!(caseless_contains("Hello, world!", "hello"));
        assert!(caseless_contains("Hello, world!", "world"));
        assert!(caseless_contains("Hello, World!", "world"));
        assert!(caseless_contains("Hello, WoRld!", "world"));

        assert!(!caseless_contains("Hello, WoRld!", "foo"));
        assert!(!caseless_contains("Hello, WoRld!", "WORLD"));
    }

    #[test]
    fn test_codegen() {
        let sql = "SELECT * FROM users WHERE value=$1; SELECT * FROM posts;";
        let code = sql_to_code(sql, &[String::from("value")]);
        println!("{}", code);
    }
}
