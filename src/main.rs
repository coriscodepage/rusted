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
}
