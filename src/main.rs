use std::io;

use crate::repl::Repl;

mod buffer;
mod repl;
mod state;

fn main() {
    let stdio = io::stdin();
    let input = stdio.lock();
    let output = io::stdout();
    Repl::begin(input, output).expect("REPL encountered a fatal error, bailing.");
}

#[cfg(test)]
mod tests {
    use super::*; // Importuje strukturę Repl i inne potrzebne elementy
    use crate::state::{Address, CommandKind, Line, Parser};
    use std::io::Cursor;
    use std::{env, fs, process};

    /// Funkcja pomocnicza do uruchamiania Repl z udawanym wejściem (mockiem)
    fn run_mock(input_data: &str) -> String {
        let mut input = Cursor::new(input_data.as_bytes());
        let mut output = Vec::new();

        // Ignorujemy błędy Result dla uproszczenia testów,
        // ale sprawdzamy, czy Repl nie panikuje.
        let _ = Repl::begin(&mut input, &mut output);

        String::from_utf8(output).expect("Output to nie jest poprawne UTF-8")
    }

    fn listed_lines(output: &str) -> Vec<String> {
        output
            .replace("\r\n", "\n")
            .lines()
            .filter(|line| line.ends_with('$'))
            .map(ToOwned::to_owned)
            .collect()
    }

    fn numbered_lines(output: &str) -> Vec<String> {
        output
            .replace("\r\n", "\n")
            .lines()
            .filter(|line| line.contains('\t'))
            .map(ToOwned::to_owned)
            .collect()
    }

    fn plain_lines(output: &str) -> Vec<String> {
        output
            .replace("\r\n", "\n")
            .lines()
            .filter(|line| !line.is_empty() && *line != "?" && !line.ends_with('$'))
            .filter(|line| !line.chars().all(|c| c.is_ascii_digit()))
            .filter(|line| !line.contains('\t'))
            .map(ToOwned::to_owned)
            .collect()
    }

    #[test]
    fn test_basic_append_and_list_range() {
        // Testuje dodawanie i wyświetlanie konkretnego zakresu
        let input = "\
a
Linia 1
Linia 2
Linia 3
.
1,2l
q
q
";
        let output = run_mock(input);

        // Powinno zawierać tylko linie 1 i 2, zakończone znakiem $
        assert!(output.contains("Linia 1$"));
        assert!(output.contains("Linia 2$"));
        assert!(
            !output.contains("Linia 3$"),
            "Linia 3 nie powinna być widoczna w zakresie 1,2"
        );
    }

    #[test]
    fn test_delete_and_index_shift() {
        // Sprawdza, czy po usunięciu środkowej linii reszta się przesuwa
        let input = "\
a
Pierwsza
Druga
Trzecia
.
2d
1,$l
q
q
";
        let output = run_mock(input);

        // Po usunięciu "Druga", "Trzecia" powinna stać się drugą linią
        assert!(output.contains("Pierwsza$"));
        assert!(output.contains("Trzecia$"));
        assert!(!output.contains("Druga$"));
    }

    #[test]
    fn test_current_line_pointer() {
        // Testuje, czy kropka (.) poprawnie śledzi ostatnią operację
        let input = "\
a
L1
L2
L3
.
1l      # Ustaw kropkę na 1
l       # Wyświetl bieżącą (powinna być L1)
3l      # Ustaw kropkę na 3
l       # Wyświetl bieżącą (powinna być L3)
q
q
";
        let output = run_mock(input);

        // Szukamy sekwencji wystąpień, aby upewnić się, że 'l' reaguje na zmianę kropki
        let occurrences: Vec<_> = output.matches("L").collect();
        assert!(occurrences.len() >= 4);
    }

    #[test]
    fn test_address_symbols_dot_and_dollar() {
        // Testuje użycie symboli . (bieżąca) i $ (ostatnia)
        let input = "\
a
A
B
C
.
2l      # kropka na B
.,$l    # wyświetl od bieżącej do końca (B, C)
q
q
";
        let output = run_mock(input);

        assert!(output.contains("B$"));
        assert!(output.contains("C$"));
        assert!(!output.contains("A$"));
    }

    #[test]
    fn test_write_and_quit_dirty_flag() {
        // Testuje, czy edytor ostrzega o braku zapisu (Dirty Buffer)
        // Zakładamy, że pierwsza próba 'q' przy zmianach zwraca '?'
        let input = "\
a
Nowa treść
.
q
q
";
        let output = run_mock(input);

        // Powinien pojawić się znak zapytania jako ostrzeżenie
        assert!(output.contains("?"));
    }

    #[test]
    fn test_empty_buffer_errors() {
        // Testuje zachowanie na całkowicie pustym edytorze
        let input = "\
l
d
w
q
";
        let output = run_mock(input);

        // Każda operacja (poza q w niektórych implementacjach) powinna rzucić błędem
        let error_count = output.matches('?').count();
        assert!(
            error_count >= 3,
            "Pusty bufor powinien generować błędy '?' dla l, d, w"
        );
    }

    #[test]
    fn test_regex_substitute_s() {
        // Dodajemy tekst, podmieniamy słowo, wyświetlamy i wychodzimy
        let input = "\
a
Rdza jest trudna
Rdza jest fajna
.
1,2s/trudna/szybka/
1,2l
q
q
";
        let output = run_mock(input);

        // Linia 1 powinna ulec zmianie, linia 2 pozostaje bez zmian (mimo braku dopasowania)
        assert!(output.contains("Rdza jest szybka$"));
        assert!(output.contains("Rdza jest fajna$"));
    }

    #[test]
    fn test_regex_invalid_pattern() {
        // Próba podania zepsutego regexu (niedomknięty nawias)
        let input = "\
a
Test
.
1s/[a-z/nowy/
q
q
";
        let output = run_mock(input);
        // Program powinien rzucić znakiem zapytania, a nie panikować
        assert!(output.contains("?"));
    }

    #[test]
    fn test_transfer_t() {
        // t kopiuje zakres linii na nowy adres
        let input = "\
a
A
B
C
.
1,2t3
1,$l
q
q
";
        let output = run_mock(input);

        // Oczekujemy: A, B, C, A, B
        let expected = "A$\nB$\nC$\nA$\nB$";
        assert!(
            output.replace("\r\n", "\n").contains(expected),
            "Wynik transferu jest niepoprawny:\n{}",
            output
        );
    }

    #[test]
    fn test_cut_and_yank_x_y() {
        // y: yank (kopiuje do rejestru, nie usuwa z bufora)
        let input = "\
a
Linia 1
Linia 2
Linia 3
.
2y
2x
1,$l
q
q
";
        let output = run_mock(input);
        // Bufor powinien teraz zawierać tylko: Linia 1 i Linia 3.
        assert!(output.contains("Linia 1$"));
        assert!(output.lines().filter(|v| v.contains("Linia 2$")).count() == 2,);
        assert!(output.contains("Linia 3$"));
    }

    #[test]
    fn test_file_read_and_edit_e_r() {
        // Przygotowujemy tymczasowy plik do testów
        let filename = "mock_test_r_e.txt";
        fs::write(filename, "Zewnetrzne dane\nKoniec danych\n").unwrap();

        // r: dołączenie pliku, e: edycja (zastąpienie) pliku
        let input = format!(
            "\
a
Stare dane
.
$r {filename}
1,$l
e {filename}
1,$l
q
q
"
        );
        let output = run_mock(&input);
        // Najpierw 'r' powinno dołączyć linie z pliku na koniec
        assert!(output.contains("Stare dane$"));
        assert!(output.contains("Zewnetrzne dane$"));

        // Następnie 'e' powinno zresetować bufor i wczytać tylko plik
        // Sprawdzamy stan po drugim 1,$l (po komendzie e)
        let parts: Vec<&str> = output.split("30").collect();
        let binding = output.as_str();
        let last_part = parts.last().unwrap_or(&binding);

        assert!(
            !last_part.contains("Stare dane$"),
            "Komenda 'e' nie wyczyściła starego bufora!"
        );
        assert!(last_part.contains("Zewnetrzne dane$"));

        // Sprzątamy plik testowy
        let _ = fs::remove_file(filename);
    }

    #[test]
    fn test_bad_addressing_with_new_commands() {
        // Sprawdzamy czy komendy na zlych adresach odpowiadaja '?'
        let input = "\
1,5t2
1x
2,1s/a/b/
q
q
";
        let output = run_mock(input);

        // Powinniśmy dostać znak zapytania za każde wywołanie
        assert_eq!(
            output.matches('?').count(),
            3,
            "Brakujące znaki błędu '?':\n{}",
            output
        );
    }

    #[test]
    fn test_insert_before_address_and_numbered_range() {
        let input = "\
a
alpha
beta
gamma
.
2i
before beta
.
1,$n
q
q
";
        let output = run_mock(input);

        assert_eq!(
            numbered_lines(&output),
            vec!["1\talpha", "2\tbefore beta", "3\tbeta", "4\tgamma"]
        );
    }

    #[test]
    fn test_change_yanks_deleted_range_for_later_put() {
        let input = "\
a
one
two
three
four
.
2,3c
TWO-THREE
.
$x
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(
            listed_lines(&output),
            vec!["one$", "TWO-THREE$", "four$", "two$", "three$"]
        );
    }

    #[test]
    fn test_undo_toggles_last_mutation() {
        let input = "\
a
one
two
three
.
2d
u
1,$l
u
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(
            listed_lines(&output),
            vec!["one$", "two$", "three$", "one$", "three$"]
        );
    }

    #[test]
    fn test_move_range_after_destination() {
        let input = "\
a
A
B
C
D
.
2,3m$
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["A$", "D$", "B$", "C$"]);
    }

    #[test]
    fn test_move_rejects_destination_inside_range_without_changing_buffer() {
        let input = "\
a
A
B
C
D
.
2,3m2
1,$l
q
q
";
        let output = run_mock(input);

        assert!(
            output.contains('?'),
            "expected invalid move to report an error"
        );
        assert_eq!(listed_lines(&output), vec!["A$", "B$", "C$", "D$"]);
    }

    #[test]
    fn test_join_single_line_noop_then_multi_line_join() {
        let input = "\
a
red
green
blue
.
2j
1,3j
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["redgreenblue$"]);
    }

    #[test]
    fn test_substitution_nth_global_captures_and_ampersand() {
        let input = "\
a
abc abc abc
x=12 y=34
.
1s/abc/[&]/2
2s/([a-z])=([0-9]+)/\\1:\\2/g
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["abc [abc] abc$", "x:12 y:34$"]);
    }

    #[test]
    fn test_regex_addresses_wrap_and_backward_search() {
        let input = "\
a
alpha
beta
gamma
beta
.
4l
/alpha/l
?beta?l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["beta$", "alpha$", "beta$"]);
    }

    #[test]
    fn test_semicolon_range_uses_first_address_as_current() {
        let input = "\
a
A
B
C
D
.
2;.+1l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["B$", "C$"]);
    }

    #[test]
    fn test_append_suffix_runs_after_input_mode_finishes() {
        let input = "\
al
one
two
.
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["two$"]);
    }

    #[test]
    fn test_write_saves_whole_buffer_and_clears_dirty_quit_warning() {
        let path = env::temp_dir().join(format!("ed_write_{}.txt", process::id()));
        let filename = path.to_string_lossy();
        let input = format!(
            "\
a
save me
second line
.
w {filename}
q
"
        );

        let output = run_mock(&input);
        let contents = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(contents, "save me\nsecond line\n");
        assert!(
            !output.contains('?'),
            "write should clear the dirty quit warning: {output}"
        );
    }

    #[test]
    fn test_parser_accepts_percent_comma_semicolon_and_offsets() {
        let mut parser = Parser::new();

        let percent = parser.parse("%l").unwrap();
        assert_eq!(
            percent.address,
            Address::Range(Line::First(0), Line::Last(0))
        );
        assert!(matches!(percent.kind, CommandKind::List));

        let omitted_start = parser.parse(",2p").unwrap();
        assert_eq!(
            omitted_start.address,
            Address::Range(Line::First(0), Line::Absolute(2, 0))
        );
        assert!(matches!(omitted_start.kind, CommandKind::PrintList));

        let semicolon_default = parser.parse(";n").unwrap();
        assert_eq!(
            semicolon_default.address,
            Address::RangeSemicolon(Line::Current(0), Line::Last(0))
        );
        assert!(matches!(semicolon_default.kind, CommandKind::NumberedList));

        let combined_offsets = parser.parse("1+2-1l").unwrap();
        assert_eq!(
            combined_offsets.address,
            Address::Single(Line::Absolute(1, 1))
        );
        assert!(matches!(combined_offsets.kind, CommandKind::List));
    }

    #[test]
    fn test_parser_rejects_suffix_on_filename_commands_without_space() {
        let mut parser = Parser::new();

        assert!(parser.parse("wfile.txt").is_err());
        assert!(parser.parse("efile.txt").is_err());
        assert!(parser.parse("rfile.txt").is_err());
    }

    #[test]
    fn test_percent_and_omitted_comma_addresses_in_repl() {
        let input = "\
a
A
B
C
D
.
%l
,2l
3,l
q
q
";
        let output = run_mock(input);

        assert_eq!(
            listed_lines(&output),
            vec!["A$", "B$", "C$", "D$", "A$", "B$", "C$"]
        );
    }

    #[test]
    fn test_address_offsets_and_address_only_print_command() {
        let input = "\
a
one
two
three
four
.
1+2l
$-2l
3
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["three$", "two$"]);
        assert_eq!(plain_lines(&output), vec!["three"]);
    }

    #[test]
    fn test_list_escapes_tabs_and_backslashes() {
        let input = "\
a
col\tvalue
path\\name
.
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["col\\tvalue$", "path\\\\name$"]);
    }

    #[test]
    fn test_delete_and_substitution_suffixes_print_affected_line() {
        let input = "\
a
alpha
beta
gamma
.
2dp
1s/alpha/ALPHA/p
q
q
";
        let output = run_mock(input);

        assert_eq!(plain_lines(&output), vec!["gamma", "ALPHA"]);
    }

    #[test]
    fn test_put_and_undo_without_saved_data_report_errors() {
        let input = "\
u
x
q
";
        let output = run_mock(input);

        assert_eq!(output.matches('?').count(), 2);
    }

    #[test]
    fn test_zero_address_appends_and_transfers_before_first_line() {
        let input = "\
0a
zero
.
a
one
two
.
2t0
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["one$", "zero$", "one$", "two$"]);
    }

    #[test]
    fn test_move_range_before_destination() {
        let input = "\
a
A
B
C
D
E
.
3,4m1
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["A$", "C$", "D$", "B$", "E$"]);
    }

    #[test]
    fn test_empty_regex_address_reuses_previous_regex_forward_and_backward() {
        let input = "\
a
alpha
beta
gamma
beta
.
/gamma/l
//-1l
??l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["gamma$", "beta$", "gamma$"]);
    }

    #[test]
    fn test_substitution_empty_regex_reuses_previous_regex() {
        let input = "\
a
red fish
blue fish
.
/fish/l
1,2s//bird/g
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(
            listed_lines(&output),
            vec!["red fish$", "red bird$", "blue bird$"]
        );
    }

    #[test]
    fn test_substitution_zero_flag_is_error_and_leaves_line_unchanged() {
        let input = "\
a
foo foo
.
1s/foo/bar/0
1l
q
q
";
        let output = run_mock(input);

        assert!(output.contains('?'));
        assert_eq!(listed_lines(&output), vec!["foo foo$"]);
    }

    #[test]
    fn test_write_explicit_range_only() {
        let path = env::temp_dir().join(format!("ed_write_range_{}.txt", process::id()));
        let filename = path.to_string_lossy();
        let input = format!(
            "\
a
alpha
beta
gamma
.
2,3w {filename}
q
q
"
        );

        let output = run_mock(&input);
        let contents = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(contents, "beta\ngamma\n");
        assert!(output.contains("11"));
    }

//     #[test]
//     fn test_read_at_zero_and_undo_restores_empty_buffer() {
//         let path = env::temp_dir().join(format!("ed_read_undo_{}.txt", process::id()));
//         let filename = path.to_string_lossy();
//         fs::write(&path, "from file\nsecond\n").unwrap();
//         let input = format!(
//             "\
// 0r {filename}
// 1,$l
// u
// l
// q
// "
//         );

//         let output = run_mock(&input);
//         let _ = fs::remove_file(&path);

//         assert_eq!(listed_lines(&output), vec!["from file$", "second$"]);
//         assert!(
//             output.contains('?'),
//             "listing after undoing the read should report an empty-buffer error"
//         );
//     }

    #[test]
    fn test_insert_zero_address_places_text_at_start() {
        let input = "\
a
one
two
.
0i
zero
.
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["zero$", "one$", "two$"]);
    }

    #[test]
    fn test_change_with_zero_address_errors() {
        let input = "\
a
old first
second
.
0c
q
q
";
        let output = run_mock(input);

        assert_eq!(output.matches('?').count(), 2);
    }

    #[test]
    fn test_change_with_no_input_deletes_range_and_current_moves_after_deleted_lines() {
        let input = "\
a
one
two
three
.
2c
.
p
1,$l
q
q
";
        let output = run_mock(input);
        assert_eq!(plain_lines(&output), vec!["three"]);
        assert_eq!(listed_lines(&output), vec!["one$", "three$"]);
    }

    #[test]
    fn test_change_suffix_number_prints_new_current_line() {
        let input = "\
a
one
two
three
.
2cn
TWO
.
q
q
";
        let output = run_mock(input);

        assert_eq!(numbered_lines(&output), vec!["2\tTWO"]);
    }

    #[test]
    fn test_default_join_uses_current_and_next_line() {
        let input = "\
a
red
green
blue
.
1l
j
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["red$", "redgreen$", "blue$"]);
    }

    #[test]
    fn test_semicolon_relative_range_join_uses_first_address_as_current() {
        let input = "\
a
A
B
C
D
.
2;.+1j
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["A$", "BC$", "D$"]);
    }

    #[test]
    fn test_move_to_zero_places_range_at_beginning() {
        let input = "\
a
A
B
C
D
.
3,4m0
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["C$", "D$", "A$", "B$"]);
    }

    #[test]
    fn test_yank_range_and_put_at_zero_copies_without_deleting_original() {
        let input = "\
a
A
B
C
D
.
2,3y
0x
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(
            listed_lines(&output),
            vec!["B$", "C$", "A$", "B$", "C$", "D$"]
        );
    }

    #[test]
    fn test_substitution_without_match_reports_error_and_preserves_current_line() {
        let input = "\
a
alpha
beta
.
2l
1s/gamma/GAMMA/
p
1,$l
q
q
";
        let output = run_mock(input);
        assert!(output.contains('?'));
        assert_eq!(plain_lines(&output), vec!["beta"]);
        assert_eq!(listed_lines(&output), vec!["beta$", "alpha$", "beta$"]);
    }

    #[test]
    fn test_undo_restores_substitution() {
        let input = "\
a
alpha
beta
.
1s/alpha/ALPHA/
u
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["alpha$", "beta$"]);
    }

    #[test]
    fn test_undo_restores_join() {
        let input = "\
a
red
green
blue
.
1,2j
u
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["red$", "green$", "blue$"]);
    }

    #[test]
    fn test_read_without_filename_reuses_current_filename() {
        let path = env::temp_dir().join(format!("ed_read_current_{}.txt", process::id()));
        let filename = path.to_string_lossy();
        fs::write(&path, "external\n").unwrap();
        let input = format!(
            "\
0r {filename}
r
1,$l
q
q
"
        );

        let output = run_mock(&input);
        let _ = fs::remove_file(&path);

        assert_eq!(listed_lines(&output), vec!["external$", "external$"]);
    }

    #[test]
    fn test_edit_remembers_filename_for_later_write_without_argument() {
        let path = env::temp_dir().join(format!("ed_edit_current_{}.txt", process::id()));
        let filename = path.to_string_lossy();
        fs::write(&path, "original\n").unwrap();
        let input = format!(
            "\
e {filename}
a
added
.
w
q
q
"
        );

        let output = run_mock(&input);
        let contents = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert!(!output.contains('?'));
        assert_eq!(contents, "originaladded\n");
    }

    #[test]
    fn test_write_does_not_replace_existing_current_filename() {
        let first = env::temp_dir().join(format!("ed_current_first_{}.txt", process::id()));
        let second = env::temp_dir().join(format!("ed_current_second_{}.txt", process::id()));
        let first_name = first.to_string_lossy();
        let second_name = second.to_string_lossy();
        let input = format!(
            "\
a
payload
.
w {first_name}
1s/payload/updated/
w {second_name}
1s/updated/final/
w
q
"
        );

        let output = run_mock(&input);
        let first_contents = fs::read_to_string(&first).unwrap();
        let second_contents = fs::read_to_string(&second).unwrap();
        let _ = fs::remove_file(&first);
        let _ = fs::remove_file(&second);
        assert!(!output.contains('?'));
        assert_eq!(first_contents, "final\n");
        assert_eq!(second_contents, "updated\n");
    }

    #[test]
    fn test_filename_command_prints_name_remembered_by_edit() {
        let path = env::temp_dir().join(format!("ed_f_print_{}.txt", process::id()));
        let filename = path.to_string_lossy();
        fs::write(&path, "loaded\n").unwrap();
        let input = format!(
            "\
e {filename}
f
q
"
        );

        let output = run_mock(&input);
        let _ = fs::remove_file(&path);

        assert_eq!(plain_lines(&output), vec![filename.to_string()]);
    }

    #[test]
    fn test_filename_command_sets_default_for_write_and_prints_new_name() {
        let path = env::temp_dir().join(format!("ed_f_set_{}.txt", process::id()));
        let filename = path.to_string_lossy();
        let input = format!(
            "\
a
saved through f
.
f {filename}
w
q
"
        );

        let output = run_mock(&input);
        let contents = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(contents, "saved through f\n");
        assert_eq!(plain_lines(&output), vec![filename.to_string()]);
    }

    #[test]
    fn test_filename_command_does_not_change_current_line() {
        let path = env::temp_dir().join(format!("ed_f_current_{}.txt", process::id()));
        let filename = path.to_string_lossy();
        let input = format!(
            "\
a
first
second
third
.
2p
f {filename}
p
q
q
"
        );

        let output = run_mock(&input);
        let _ = fs::remove_file(&path);

        assert_eq!(
            plain_lines(&output),
            vec!["second", filename.as_ref(), "second"]
        );
    }

    #[test]
    fn test_filename_command_without_remembered_name_reports_error() {
        let input = "\
f
q
";
        let output = run_mock(input);

        assert_eq!(output.matches('?').count(), 1);
    }

    #[test]
    fn test_substitution_percent_reuses_previous_replacement_and_escaped_percent_is_literal() {
        let input = "\
a
cat dog
bird dog
percent dog
.
1s/dog/wolf/
2s/dog/%/
3s/dog/\\%/
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(
            listed_lines(&output),
            vec!["cat wolf$", "bird wolf$", "percent %$"]
        );
    }

    #[test]
    fn test_substitution_ampersand_and_escaped_ampersand() {
        let input = "\
a
abc 123
abc 456
.
1s/[0-9][0-9]*/<&>/
2s/[0-9][0-9]*/\\&/
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["abc <123>$", "abc &$"]);
    }

    #[test]
    fn test_substitution_percent_without_previous_replacement_is_error() {
        let input = "\
a
lonely token
.
1s/token/%/
1l
q
q
";
        let output = run_mock(input);

        assert!(output.contains('?'));
        assert_eq!(listed_lines(&output), vec!["lonely token$"]);
    }

    #[test]
    fn test_substitution_percent_in_longer_replacement_is_literal() {
        let input = "\
a
load 50
.
1s/50/50% done/
1l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["load 50% done$"]);
    }

    #[test]
    fn test_substitution_supports_nonstandard_separator() {
        let input = "\
a
path usr/bin
.
1s#usr/bin#opt/bin#
1l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["path opt/bin$"]);
    }

    #[test]
    fn test_substitution_allows_escaped_nonstandard_separator_in_regex() {
        let input = "\
a
literal a#b marker
.
1s#a\\#b#hash#
1l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["literal hash marker$"]);
    }

    #[test]
    fn test_regex_line_addresses_support_offsets() {
        let input = "\
a
alpha
beta
gamma
beta
delta
.
/beta/+1l
?beta?-1l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["gamma$", "alpha$"]);
    }

    #[test]
    fn test_regex_line_address_offsets_in_semicolon_range() {
        let input = "\
a
A
B
C
D
E
.
/B/;/D/-1l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["B$", "C$"]);
    }

    #[test]
    fn test_substitution_default_global_and_numeric_flags() {
        let input = "\
a
foo foo foo
foo foo foo
foo foo foo
.
1s/foo/bar/
2s/foo/bar/g
3s/foo/bar/3
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(
            listed_lines(&output),
            vec!["bar foo foo$", "bar bar bar$", "foo foo bar$"]
        );
    }

    #[test]
    fn test_substitution_multidigit_numeric_flag_replaces_that_match_only() {
        let input = "\
a
x x x x x x x x x x x
.
1s/x/y/10
1l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["x x x x x x x x x y x$"]);
    }

    #[test]
    fn test_substitution_numeric_flag_too_large_is_error_without_change() {
        let input = "\
a
foo foo
.
1s/foo/bar/3
1l
q
q
";
        let output = run_mock(input);

        assert!(output.contains('?'));
        assert_eq!(listed_lines(&output), vec!["foo foo$"]);
    }

    #[test]
    fn test_substitution_omitted_final_delimiter_prints_last_affected_line() {
        let input = "\
a
foo one
foo two
.
1s/foo/bar
2s/foo/baz/
q
q
";
        let output = run_mock(input);

        assert_eq!(plain_lines(&output), vec!["bar one"]);
    }

    #[test]
    fn test_substitution_omitted_replacement_and_final_delimiters_use_empty_replacement_and_print()
    {
        let input = "\
a
foo tail
.
1s/foo
1l
q
q
";
        let output = run_mock(input);

        assert_eq!(plain_lines(&output), vec!["tail"]);
        assert_eq!(listed_lines(&output), vec![" tail$"]);
    }

    #[test]
    fn test_substitution_embedded_newline_in_replacement_splits_line() {
        let input = "\
a
left right
.
1s/ /\\
/
1,$l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["left$", "right$"]);
    }

    #[test]
    fn test_substitution_rejects_blank_or_newline_delimiters() {
        let input = "\
a
foo
.
1s foo bar
1s\tfoo\tbar
1s
1l
q
q
";
        let output = run_mock(input);

        assert_eq!(output.matches('?').count(), 4);
        assert_eq!(listed_lines(&output), vec!["foo$"]);
    }

    #[test]
    fn test_address_offsets_ignore_blanks_between_terms() {
        let input = "\
a
A
B
C
D
.
1 + 2l
$ - 1l
1 - 2 + 2l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["C$", "C$", "A$"]);
    }

    #[test]
    fn test_final_address_out_of_range_is_error_after_offsets() {
        let input = "\
a
A
B
.
$+1l
1-2l
1l
q
q
";
        let output = run_mock(input);

        assert_eq!(output.matches('?').count(), 3);
        assert_eq!(listed_lines(&output), vec!["A$"]);
    }

    #[test]
    fn test_forward_and_backward_regex_address_can_omit_closing_delimiter_at_eol() {
        let input = "\
a
alpha
beta
gamma
.
/gamma
?alpha
q
q
";
        let output = run_mock(input);

        assert_eq!(plain_lines(&output), vec!["gamma", "alpha"]);
    }

    #[test]
    fn test_regex_address_escaped_delimiters_match_literals() {
        let input = "\
a
a/b
a?b
plain
.
/a\\/b/l
?a\\?b?l
q
q
";
        let output = run_mock(input);

        assert_eq!(listed_lines(&output), vec!["a/b$", "a?b$"]);
    }

    #[test]
    fn test_regex_address_not_found_is_error_and_current_line_is_preserved() {
        let input = "\
a
alpha
beta
.
2p
/missing/l
p
q
q
";
        let output = run_mock(input);

        assert!(output.contains('?'));
        assert_eq!(plain_lines(&output), vec!["beta", "beta"]);
    }
}
