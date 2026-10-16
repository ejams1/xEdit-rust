// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// The `TPerlRegEx` probe of `cargo xtask parity script --script "Regex
// probe" --phase run`: it runs over an empty master on the GUI build and on
// the port, adds one `name=value` line per probe to the message log and
// writes the same lines to `Regex probe.txt` next to the script, so the two
// sides are compared as log lines and as a file.
//
// A value holds no byte outside ASCII: a probe over a multi-byte subject
// prints its offsets and lengths (and boolean comparisons against the
// expected text), never the text itself, so the code page the GUI writes
// its log in cannot change what is compared. JvInterpreter has no string
// indexing (`s[1]` is `Type mismatch`) and `Ord` takes no string, so
// character codes are not available to a script.

unit RegexProbe;

var
  GLog: TStringList;
  GPath: string;
  GCount: Integer;

// Every line lands in the message log and in the results file; the file is
// written every 20 lines and at every section, because each write is far
// more expensive than the probe itself.
procedure Flush;
begin
  if GCount mod 20 = 0 then
    GLog.SaveToFile(GPath);
end;

procedure P(aName: string; aValue: string);
begin
  GCount := GCount + 1;
  AddMessage(aName + '=' + aValue);
  GLog.Add(aName + '=' + aValue);
  Flush;
end;

// A section marker: the results file only, not the message log.
procedure Mark(aName: string);
begin
  GLog.SaveToFile(GPath);
  GLog.Add('--' + aName);
  GLog.SaveToFile(GPath);
end;

function BoolStr(aValue: Boolean): string;
begin
  if aValue then
    Result := 'true'
  else
    Result := 'false';
end;

// The matches of an expression without the matched text: the count and each
// match's offset and length. A multi-byte subject is probed with this.
function WalkN(aPattern: string; aSubject: string; aOptions: Integer; aCap: Integer): string;
var
  r: TPerlRegEx;
  n: Integer;
begin
  Result := '';
  r := TPerlRegEx.Create;
  try
    try
      r.Subject := aSubject;
      r.RegEx := aPattern;
      r.Options := aOptions;
      r.Compile;
      n := 0;
      while r.MatchAgain do begin
        n := n + 1;
        if n > 1 then
          Result := Result + ';';
        Result := Result + IntToStr(r.MatchedOffset) + '+' + IntToStr(r.MatchedLength);
        if n >= aCap then begin
          Result := Result + ';cap';
          Break;
        end;
      end;
      Result := IntToStr(n) + '[' + Result + ']';
    except
      on E: Exception do
        Result := 'raise:' + E.Message;
    end;
  finally
    r.Free;
  end;
end;

// [`WalkN`] with the matched text, for ASCII subjects.
function WalkT(aPattern: string; aSubject: string; aOptions: Integer; aCap: Integer): string;
var
  r: TPerlRegEx;
  n: Integer;
begin
  Result := '';
  r := TPerlRegEx.Create;
  try
    try
      r.Subject := aSubject;
      r.RegEx := aPattern;
      r.Options := aOptions;
      r.Compile;
      n := 0;
      while r.MatchAgain do begin
        n := n + 1;
        if n > 1 then
          Result := Result + ';';
        Result := Result + IntToStr(r.MatchedOffset) + '+' + IntToStr(r.MatchedLength) + ':' + r.MatchedText;
        if n >= aCap then begin
          Result := Result + ';cap';
          Break;
        end;
      end;
      Result := IntToStr(n) + '[' + Result + ']';
    except
      on E: Exception do
        Result := 'raise:' + E.Message;
    end;
  finally
    r.Free;
  end;
end;

// `nomatch` or the match's offset, length and text: `MatchedText` raises
// when there is no match, so nothing reads it unguarded.
function Info(aRegEx: TPerlRegEx; aFound: Boolean): string;
begin
  if aFound then
    Result := IntToStr(aRegEx.MatchedOffset) + '+' + IntToStr(aRegEx.MatchedLength) + ':' + aRegEx.MatchedText
  else
    Result := 'nomatch';
end;

// The groups of the first match without texts: the group count, then each
// group's offset and length.
function Groups(aPattern: string; aSubject: string; aOptions: Integer): string;
var
  r: TPerlRegEx;
  i: Integer;
begin
  Result := '';
  r := TPerlRegEx.Create;
  try
    try
      r.Subject := aSubject;
      r.RegEx := aPattern;
      r.Options := aOptions;
      r.Compile;
      if not r.MatchAgain then begin
        Result := 'nomatch';
        Exit;
      end;
      Result := 'count=' + IntToStr(r.GroupCount);
      for i := 0 to r.GroupCount do
        Result := Result + ' g' + IntToStr(i) + '=' + IntToStr(r.GroupOffsets[i]) + '+'
          + IntToStr(r.GroupLengths[i]);
      Result := Result + ' text=' + IntToStr(r.MatchedLength) + ' at=' + IntToStr(r.MatchedOffset);
    except
      on E: Exception do
        Result := 'raise:' + E.Message;
    end;
  finally
    r.Free;
  end;
end;

// [`Groups`] with the texts, for ASCII subjects.
function GroupsT(aPattern: string; aSubject: string; aOptions: Integer): string;
var
  r: TPerlRegEx;
  i: Integer;
begin
  Result := '';
  r := TPerlRegEx.Create;
  try
    try
      r.Subject := aSubject;
      r.RegEx := aPattern;
      r.Options := aOptions;
      r.Compile;
      if not r.MatchAgain then begin
        Result := 'nomatch';
        Exit;
      end;
      Result := 'count=' + IntToStr(r.GroupCount);
      for i := 0 to r.GroupCount do
        Result := Result + ' g' + IntToStr(i) + '=' + IntToStr(r.GroupOffsets[i]) + '+'
          + IntToStr(r.GroupLengths[i]) + ':' + r.Groups[i];
      Result := Result + ' text=' + r.MatchedText + ' at=' + IntToStr(r.MatchedOffset);
    except
      on E: Exception do
        Result := 'raise:' + E.Message;
    end;
  finally
    r.Free;
  end;
end;

// The message `Compile` raises for a pattern and its options, or `ok`.
function CompileErr(aPattern: string; aOptions: Integer): string;
var
  r: TPerlRegEx;
begin
  Result := 'ok';
  r := TPerlRegEx.Create;
  try
    r.RegEx := aPattern;
    r.Options := aOptions;
    try
      r.Compile;
    except
      on E: Exception do
        Result := E.Message;
    end;
  finally
    r.Free;
  end;
end;

// `ReplaceAll` over an ASCII subject: whether it matched and the subject
// afterwards.
function Replaced(aPattern: string; aSubject: string; aReplacement: string; aOptions: Integer): string;
var
  r: TPerlRegEx;
begin
  Result := '';
  r := TPerlRegEx.Create;
  try
    try
      r.Subject := aSubject;
      r.RegEx := aPattern;
      r.Replacement := aReplacement;
      r.Options := aOptions;
      r.Compile;
      Result := BoolStr(r.ReplaceAll) + ':' + r.Subject;
    except
      on E: Exception do
        Result := 'raise:' + E.Message;
    end;
  finally
    r.Free;
  end;
end;

// `ReplaceAll` over any subject: whether it matched, the character count of
// the subject afterwards and whether it is the expected text.
function ReplacedCmp(aPattern: string; aSubject: string; aReplacement: string; aOptions: Integer; aExpect: string): string;
var
  r: TPerlRegEx;
begin
  Result := '';
  r := TPerlRegEx.Create;
  try
    try
      r.Subject := aSubject;
      r.RegEx := aPattern;
      r.Replacement := aReplacement;
      r.Options := aOptions;
      r.Compile;
      Result := BoolStr(r.ReplaceAll) + ':len=' + IntToStr(Length(r.Subject)) + ':eq=' + BoolStr(r.Subject = aExpect);
    except
      on E: Exception do
        Result := 'raise:' + E.Message;
    end;
  finally
    r.Free;
  end;
end;

// `ComputeReplacement` of the first match, as the replacement text.
function Computed(aPattern: string; aSubject: string; aReplacement: string; aOptions: Integer): string;
var
  r: TPerlRegEx;
begin
  Result := '';
  r := TPerlRegEx.Create;
  try
    try
      r.Subject := aSubject;
      r.RegEx := aPattern;
      r.Replacement := aReplacement;
      r.Options := aOptions;
      r.Compile;
      if not r.MatchAgain then begin
        Result := 'nomatch';
        Exit;
      end;
      Result := r.ComputeReplacement;
    except
      on E: Exception do
        Result := 'raise:' + E.Message;
    end;
  finally
    r.Free;
  end;
end;

// ---------------------------------------------------------------- probes

// `a`, `e-acute` (U+00E9: two UTF-8 bytes, one Delphi character) and `z`.
function MultiByte: string;
begin
  Result := 'a' + Chr(233) + 'z';
end;

procedure Bytes;
begin
  P('dot-multibyte', WalkN('.', MultiByte, 0, 10));
  P('dot-ascii', WalkT('.', 'ab', 0, 10));
  P('dot-nl', WalkN('.', 'a' + Chr(10) + 'b', 0, 10));
  P('dot-singleline', WalkN('.', 'a' + Chr(10) + 'b', 4, 10));
  P('literal-multibyte', WalkN(Chr(233), MultiByte, 0, 10));
  P('class-multibyte', WalkN('[^a]', MultiByte, 0, 10));
  P('groups-multibyte', Groups('(a(.))z', MultiByte, 0));
  P('groups-ascii', GroupsT('(a(b))z', 'abz', 0));
  P('word-ascii-multibyte', WalkN('\w', MultiByte, 0, 10));
  P('notword-ascii-multibyte', WalkN('\W', MultiByte, 0, 10));
  P('digit-ascii-arabic', WalkN('\d', 'a1' + Chr(1633) + '2', 0, 10));
  P('space-ascii-nbsp', WalkN('\s', 'a' + Chr(160) + ' b', 0, 10));
  P('boundary-multibyte', WalkN('\b', MultiByte, 0, 10));
  P('posix-digit-arabic', WalkN('[[:digit:]]', 'a1' + Chr(1633) + '2', 0, 10));
  P('posix-alpha-multibyte', WalkN('[[:alpha:]]', MultiByte, 0, 10));
  P('range-multibyte', WalkN('[a-z]', MultiByte, 0, 10));
  P('class-literal-multibyte', WalkN('[' + Chr(233) + ']', Chr(233) + 'x', 0, 10));
  P('caseless-multibyte', WalkN(Chr(233), Chr(201) + Chr(233), 1, 10));
  P('caseless-class-ascii', WalkT('[a-z]', 'AZ', 1, 10));
  P('caseless-class-multibyte', WalkN('[a-z]', Chr(201) + Chr(233), 1, 10));
  P('caseless-ascii', WalkT('abc', 'ABC', 1, 10));
  P('replace-multibyte', ReplacedCmp('.', MultiByte, '-', 0, '---'));
  P('replace-multibyte-char', ReplacedCmp('[' + Chr(233) + ']', MultiByte, '-', 0, 'a-z'));
  P('replace-multibyte-dot-star', ReplacedCmp('.*', MultiByte, '-', 0, '-'));
end;

procedure Anchors;
begin
  P('dollar-final-nl', WalkT('a$', 'a' + Chr(10), 0, 10));
  P('dollar-final-nl-multi', WalkT('a$', 'a' + Chr(10), 2, 10));
  P('dollar-final-2nl', WalkT('a$', 'a' + Chr(10) + Chr(10), 0, 10));
  P('Z-final-nl', WalkT('a\Z', 'a' + Chr(10), 0, 10));
  P('z-final-nl', WalkT('a\z', 'a' + Chr(10), 0, 10));
  P('caret-multi', WalkT('^b', 'a' + Chr(10) + 'b', 2, 10));
  P('caret-nomulti', WalkT('^b', 'a' + Chr(10) + 'b', 0, 10));
  P('A-nomulti', WalkT('\Ab', 'a' + Chr(10) + 'b', 2, 10));
  P('anchored-option', WalkT('b', 'ab', 16, 10));
  P('anchored-option-at0', WalkT('a', 'ab', 16, 10));
  P('boundary', WalkT('\bb\b', 'a b cd', 0, 10));
  P('boundary-digits', WalkT('\b\d+\b', 'x 12y 34', 0, 10));
end;

procedure Flags;
begin
  P('extended', WalkT('a b # comment', 'ab', 8, 10));
  P('extended-literal-space', WalkT('a\ b', 'a b', 8, 10));
  P('ungreedy', WalkT('a.*b', 'axbxb', 32, 10));
  P('greedy', WalkT('a.*b', 'axbxb', 0, 10));
  P('no-auto-capture', Groups('(a)(b)', 'ab', 64));
  P('no-auto-capture-group', Groups('(?:(a))(b)', 'ab', 64));
  P('inline-case', WalkT('(?i)abc', 'ABC', 0, 10));
  P('inline-off', WalkT('(?i)a(?-i)bc', 'ABc', 0, 10));
  P('inline-multi', WalkT('(?m)^b', 'a' + Chr(10) + 'b', 0, 10));
  P('inline-single', WalkT('(?s)a.b', 'a' + Chr(10) + 'b', 0, 10));
  P('inline-extended', WalkT('(?x)a b', 'ab', 0, 10));
  P('inline-ungreedy', WalkT('(?U)a.*b', 'axbxb', 0, 10));
  P('inline-scoped', WalkT('(?i:ab)c', 'ABc', 0, 10));
end;

procedure Syntax;
begin
  P('lookahead', WalkT('a(?=b)', 'ab ac', 0, 10));
  P('lookahead-neg', WalkT('a(?!b)', 'ab ac', 0, 10));
  P('lookbehind', WalkT('(?<=a)b', 'ab cb', 0, 10));
  P('lookbehind-neg', WalkT('(?<!a)b', 'ab cb', 0, 10));
  P('lookbehind-alt', WalkT('(?<=a|b)c', 'ac bc', 0, 10));
  P('lookbehind-variable', CompileErr('(?<=a+)b', 0));
  P('lookbehind-bounded', WalkT('(?<=ab{1,2})c', 'abbc', 0, 10));
  P('backref-in-pattern', WalkT('(a)\1', 'aa ab', 0, 10));
  P('backref-two-digit', WalkT('(a)(b)\2\1', 'abba', 0, 10));
  P('backref-named', WalkT('(?<x>a)\k<x>', 'aa', 0, 10));
  P('possessive-plus', WalkT('a++b', 'aab', 0, 10));
  P('possessive-star', WalkT('a*+a', 'aa', 0, 10));
  P('possessive-class', WalkT('[ab]++c', 'abbc', 0, 10));
  P('lazy-question', WalkT('a+?', 'aaa', 0, 10));
  P('lazy-braces', WalkT('a{1,3}?', 'aaa', 0, 10));
  P('atomic', WalkT('(?>a+)a', 'aa', 0, 10));
  P('atomic-alt', WalkT('(?>a|ab)c', 'abc', 0, 10));
  P('quote', WalkT('\Qa.b\E', 'a.b axb', 0, 10));
  P('named-p', Groups('(?P<n>a)(b)', 'ab', 0));
  P('named-angle', Groups('(?<n>a)(b)', 'ab', 0));
  P('named-quote', Groups('(?' + Chr(39) + 'n' + Chr(39) + 'a)(b)', 'ab', 0));
  P('conditional', WalkT('(a)?(?(1)b|c)', 'ab c', 0, 10));
  P('comment-group', WalkT('a(?#note)b', 'ab', 0, 10));
  P('hex-escape', WalkT('\x41', 'A', 0, 10));
  P('octal-escape', WalkT('\101', 'A', 0, 10));
  P('property', WalkT('\p{L}', 'a1', 0, 10));
  P('class-escape', WalkT('[\d]', 'a1', 0, 10));
  P('class-backspace', WalkT('[\b]', 'a' + Chr(8) + 'b', 0, 10));
  P('class-posix-neg', WalkT('[[:^digit:]]', 'a1', 0, 10));
  P('class-first-bracket', WalkT('[]x]', 'x]', 0, 10));
  P('class-close-literal', WalkT('[a-]', 'a-', 0, 10));
  P('alternation-order', WalkT('a|ab', 'ab', 0, 10));
  P('nested-quantifier', WalkT('(a|ab)*b', 'abb', 0, 10));
  P('greedy-group', WalkT('(a*)(a*)', 'aaa', 0, 10));
  P('repeat-empty-body', WalkT('(a?)*', 'aa', 0, 10));
  P('repeat-empty-class', WalkT('[a]*', 'aab', 0, 10));
  P('dollar-in-pattern', WalkT('a$|b', 'ab', 0, 10));
  P('brace-literal', WalkT('a{', 'a{', 0, 10));
  P('caret-mid-pattern', WalkT('a^b', 'ab', 0, 10));
  P('escape-b', WalkT('a\Bb', 'axb abb', 0, 10));
end;

procedure Invalid;
begin
  P('invalid-open-paren', CompileErr('a(', 0));
  P('invalid-open-bracket', CompileErr('a[', 0));
  P('invalid-nothing-to-repeat', CompileErr('*a', 0));
  P('invalid-quantifier-order', CompileErr('a{2,1}', 0));
  P('invalid-double-quantifier', CompileErr('a**', 0));
  P('invalid-trailing-backslash', CompileErr('\', 0));
  P('invalid-class-range', CompileErr('[z-a]', 0));
  P('invalid-empty-name', CompileErr('(?<>)', 0));
  P('invalid-duplicate-name', CompileErr('(?P<a>x)(?P<a>y)', 0));
  P('invalid-backref-two', CompileErr('(a)\2', 0));
  P('invalid-backref-none', CompileErr('\1', 0));
  P('invalid-comment', CompileErr('(?#', 0));
  P('invalid-quantifier-after-quantifier', CompileErr('a{1,2}{3}', 0));
  P('invalid-close-bracket', CompileErr(']', 0));
  P('invalid-close-paren', CompileErr('a)b', 0));
  P('invalid-inline', CompileErr('(?i', 0));
  P('invalid-open-brace', CompileErr('x{2', 0));
  P('invalid-group-name', CompileErr('(?P<1a>x)', 0));
  P('invalid-lookbehind', CompileErr('(?<=a+)b', 0));
  P('invalid-python-ref', CompileErr('(?P=1)', 0));
  P('invalid-posix', CompileErr('[[:nope:]]', 0));
  P('invalid-unknown-escape', CompileErr('\g', 0));
  P('invalid-range-order', CompileErr('[b-a]', 0));
  P('invalid-utf8-escape', CompileErr('[\x{110000}]', 0));
  P('empty-pattern', CompileErr('', 0));
  P('empty-pattern-run', WalkT('', 'abc', 0, 10));
  P('invalid-utf8-sequence', CompileErr('\x{}', 0));
  P('invalid-brace-quantifier', CompileErr('a{,2}', 0));
  P('invalid-lookbehind-order', CompileErr('(?<a)', 0));
  P('invalid-callout-arg', CompileErr('(?Cx)', 0));
end;

procedure ZeroLength;
begin
  P('star-x-replace', Replaced('x*', 'abc', '-', 0));
  P('star-b-replace', Replaced('b*', 'abc', '-', 0));
  P('plus-replace', Replaced('b+', 'abc', '-', 0));
  P('empty-walk', WalkT('x*', 'abc', 0, 10));
  P('empty-walk-cap', WalkT('', 'abc', 0, 10));
  P('alt-empty', WalkT('a|', 'ab', 0, 10));
  P('caret-walk', WalkT('^', 'abc', 0, 10));
  P('dollar-walk', WalkT('$', 'abc', 0, 10));
  P('dollar-walk-multi', WalkT('$', 'a' + Chr(10) + 'b', 2, 10));
  P('empty-class-walk', WalkT('[^x]*', 'ab', 0, 10));
  P('zero-length-group', Groups('(a*)(b*)', 'ab', 0));
end;

procedure Replacement;
begin
  P('replace-backref', Computed('(a)(b)', 'xaby', '\2\1', 0));
  P('replace-dollar', Computed('(a)(b)', 'xaby', '$2$1', 0));
  P('replace-brace', Computed('(a)(b)', 'xaby', '${1}[$2]', 0));
  P('replace-whole', Computed('a', 'abc', '<$&>', 0));
  P('replace-before', Computed('b', 'abc', '[$`]', 0));
  P('replace-after', Computed('b', 'abc', '[$' + Chr(39) + ']', 0));
  P('replace-plus', Computed('(a)(b)', 'xaby', '$+', 0));
  P('replace-dollar-dollar', Computed('a', 'abc', '$$', 0));
  P('replace-backslash-backslash', Computed('a', 'abc', '\\\\', 0));
  P('replace-unknown-group', Computed('a', 'abc', '<\3>', 0));
  P('replace-unknown-brace', Computed('a', 'abc', '<${3}>', 0));
  P('replace-named', Computed('(?P<n>a)', 'abc', '<${n}>', 0));
  P('replace-named-slash', Computed('(?P<n>a)', 'abc', '<\g<n>>', 0));
  P('replace-upper', Computed('(ab)', 'xaby', '\U1', 0));
  P('replace-lower', Computed('(AB)', 'xABy', '\L1', 0));
  P('replace-first-upper', Computed('(ab)', 'xaby', '\u1', 0));
  P('replace-trailing-backslash', Computed('b', 'abc', 'x\', 0));
  P('replace-dollar-underscore', Computed('b', 'abc', '[$_]', 0));
  P('replace-all-backref', Replaced('(a)(b)', 'xaby', '\2\1', 0));
  P('replace-all-dollar', Replaced('(a)', 'aa', '<$1>', 0));
  P('replace-all-empty-subject', Replaced('b', '', 'x', 0));
  P('replace-caseless', Replaced('b', 'aBc', 'x', 1));
  P('replace-dollar-brace-name-unknown', Computed('a', 'abc', '<${nope}>', 0));
  P('replace-ampersand-escape', Computed('a', 'abc', '\&', 0));
  P('replace-two-digit', Computed('(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)(k)', 'abcdefghijk', '\11', 0));
  P('replace-escape-upper', Computed('(a)(b)', 'xaby', '\U1\L2', 0));
  P('replace-repeat', Computed('(a)', 'a', '$1$1', 0));
end;

procedure Window;
var
  r: TPerlRegEx;
  found: Boolean;
begin
  r := TPerlRegEx.Create;
  try
    r.Subject := 'a1 b2 c3';
    r.RegEx := '[a-z][0-9]';
    r.Compile;
    found := r.Match;
    P('window-match', Info(r, found));
    found := r.MatchAgain;
    P('window-again', Info(r, found));
    r.Start := 4;
    found := r.MatchAgain;
    P('window-start', Info(r, found));
    r.Start := 0;
    r.Stop := 2;
    found := r.Match;
    P('window-stop-match', Info(r, found));
    found := r.MatchAgain;
    P('window-stop-again', Info(r, found));
    r.Stop := 0;
    r.Start := 0;
    r.Subject := '';
    P('empty-subject-match', BoolStr(r.Match));
    P('empty-subject-again', BoolStr(r.MatchAgain));
    r.Subject := 'ab';
    r.RegEx := 'b';
    r.Compile;
    found := r.Match;
    P('after-empty', BoolStr(found));
    P('found-match', BoolStr(r.FoundMatch));
    P('compiled', BoolStr(r.Compiled));
    r.Study;
    P('studied', BoolStr(r.Studied));
    P('named-unknown', IntToStr(r.NamedGroup('nope')));
    r.Subject := 'a12b';
    r.RegEx := '\d+';
    r.Compile;
    found := r.Match;
    P('after-match-start', IntToStr(r.Start));
    P('after-match-stop', IntToStr(r.Stop));
    P('after-match-offset', IntToStr(r.MatchedOffset));
    r.Subject := 'a' + Chr(233) + 'z';
    r.RegEx := 'z';
    r.Compile;
    found := r.Match;
    P('multibyte-offset', Info(r, found));
    P('multibyte-start', IntToStr(r.Start));
    P('multibyte-stop', IntToStr(r.Stop));
    try
      P('group-past-count', '[' + r.Groups[5] + ']');
    except
      on E: Exception do
        P('group-past-count', 'raise:' + E.Message);
    end;
    try
      P('offset-past-count', IntToStr(r.GroupOffsets[5]));
    except
      on E: Exception do
        P('offset-past-count', 'raise:' + E.Message);
    end;
    try
      P('length-past-count', IntToStr(r.GroupLengths[5]));
    except
      on E: Exception do
        P('length-past-count', 'raise:' + E.Message);
    end;
    r.Subject := 'abc';
    r.RegEx := 'b';
    r.Compile;
    found := r.Match;
    P('left-right', '[' + r.SubjectLeft + '][' + r.SubjectRight + ']');
    r.Subject := 'abc';
    r.RegEx := 'zzz';
    r.Compile;
    found := r.Match;
    P('left-right-nomatch', BoolStr(found) + '[' + r.SubjectLeft + '][' + r.SubjectRight + ']');
    try
      P('groups-nomatch', r.Groups[0]);
    except
      on E: Exception do
        P('groups-nomatch', 'raise:' + E.Message);
    end;
    try
      P('compute-nomatch', r.ComputeReplacement);
    except
      on E: Exception do
        P('compute-nomatch', 'raise:' + E.Message);
    end;
  finally
    r.Free;
  end;
end;

procedure Splitting;
var
  r: TPerlRegEx;
  list: TStringList;
  i: Integer;
begin
  list := TStringList.Create;
  r := TPerlRegEx.Create;
  try
    r.Subject := 'a,b,,c';
    r.RegEx := ',';
    r.Compile;
    r.Split(list, 0);
    P('split-count', IntToStr(list.Count));
    for i := 0 to list.Count - 1 do
      P('split' + IntToStr(i), list[i]);
    list.Clear;
    r.Subject := 'a,b,c,d';
    r.RegEx := ',';
    r.Split(list, 2);
    P('split-limit-count', IntToStr(list.Count));
    for i := 0 to list.Count - 1 do
      P('split-limit' + IntToStr(i), list[i]);
    list.Clear;
    r.Subject := 'a1b2c';
    r.RegEx := '([a-z])([0-9])';
    r.Compile;
    r.SplitCapture(list, 0);
    P('splitcapture-count', IntToStr(list.Count));
    for i := 0 to list.Count - 1 do
      P('splitcapture' + IntToStr(i), list[i]);
    list.Clear;
    r.Subject := 'a1b2c';
    r.RegEx := '([a-z])([0-9])';
    r.Compile;
    r.SplitCapture(list, 2);
    P('splitcapture-limit-count', IntToStr(list.Count));
    for i := 0 to list.Count - 1 do
      P('splitcapture-limit' + IntToStr(i), list[i]);
    list.Clear;
    r.Subject := 'a1b2c';
    r.RegEx := '([a-z])([0-9])';
    r.Compile;
    r.SplitCapture(list, 1);
    P('splitcapture-one-count', IntToStr(list.Count));
    for i := 0 to list.Count - 1 do
      P('splitcapture-one' + IntToStr(i), list[i]);
  finally
    r.Free;
    list.Free;
  end;
end;

procedure Storing;
var
  r: TPerlRegEx;
begin
  r := TPerlRegEx.Create;
  try
    r.Subject := 'ab';
    r.RegEx := '(a)(b)';
    r.Compile;
    r.Match;
    r.StoreGroups;
    r.Subject := 'xy';
    P('stored-g0', r.Groups[0]);
    P('stored-g1', r.Groups[1]);
    P('stored-g2', r.Groups[2]);
    P('stored-count', IntToStr(r.GroupCount));
    P('stored-text', r.MatchedText);
  finally
    r.Free;
  end;
end;

procedure Escapes;
var
  r: TPerlRegEx;
begin
  r := TPerlRegEx.Create;
  try
    P('escape-chars', r.EscapeRegExChars('a.b(c)[d]{e}|f*g?h+i^j$k-l&m<n>o'));
  finally
    r.Free;
  end;
end;

function Initialize: Integer;
begin
  Result := 0;
  GPath := ScriptsPath + 'Regex probe.txt';
  GLog := TStringList.Create;
  GLog.Add('regex-probe v1');
  GLog.SaveToFile(GPath);
  Mark('bytes');
  Bytes;
  Mark('anchors');
  Anchors;
  Mark('flags');
  Flags;
  Mark('syntax');
  Syntax;
  Mark('invalid');
  Invalid;
  Mark('zero-length');
  ZeroLength;
  Mark('replacement');
  Replacement;
  Mark('window');
  Window;
  Mark('splitting');
  Splitting;
  Mark('storing');
  Storing;
  Mark('escapes');
  Escapes;
  P('done', 'yes');
  GLog.SaveToFile(GPath);
  GLog.Free;
end;

end.
