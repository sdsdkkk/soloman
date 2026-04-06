// Soloman example: arithmetic, stdout/stderr, strings
print("Hello from Soloman");
eprint("(this line is stderr)");

let a: Int = 40 + 2;
print(a);

let s: Str = "len test: ";
print(len(s) + 3);

let line: Str = read_line();
print("You typed: " + line);
