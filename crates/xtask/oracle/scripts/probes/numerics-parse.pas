{
  Parity fixture (issue #39): the `StrToFloat` of the RTL (and the
  `dfStrToFloat` wrapper) at full precision.

  The corpus is generated from integer arithmetic so that both sides parse
  the same strings whatever their literal handling does: mantissas of
  growing digit counts, values above and below the double range, exponent
  forms over the whole range, leading and trailing blanks, malformed
  texts, and the `Inf`/`NaN` words. Every accepted value is written into
  `numerics-parse.dat` as its 8 IEEE bytes (compared byte for byte by the
  harness) and as 17-digit text into the message log; a refused text is
  logged with the exception class and message.
}
unit NumericsParseProbe;

var
  gOut: string;
  gW: TBinaryWriter;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

{ The text of a case with its control characters spelled `#<code>`: the
  message log is a memo, whose control truncates a line at a `#0` and
  breaks it at a `#10`/`#13`, and the harness reads the memo's text up to
  its first zero, so the raw `#0` cases left the oracle's run without its
  closing line (it was stopped as a hang after ten minutes). The `.dat`
  keeps the raw text. The characters are compared, not indexed: a `string`
  parameter is not indexable upstream (`Array type required`,
  `e-ordidx-*.pas`), and `Ord` of a one-character string is the variant's
  integer conversion (`Ord('7')` is 7). }
function Shown(const aText: string): string;
var
  i, k: Integer;
  c: string;
begin
  Result := '';
  for i := 1 to Length(aText) do begin
    c := Copy(aText, i, 1);
    if c < ' ' then begin
      for k := 0 to 31 do
        if c = Chr(k) then
          Result := Result + '#' + IntToStr(k);
    end else
      Result := Result + c;
  end;
end;

procedure Value(const aText: string; aValue: Double);
begin
  gW.Write('py[' + aText + ']');
  gW.Write(aValue);
  Log('py[' + Shown(aText) + ']', FloatToStrF(aValue, 1, 17, 0));
end;

procedure ParseOne(const aText: string);
var
  v: Double;
begin
  try
    v := StrToFloat(aText);
    Value(aText, v);
  except on E: Exception do
    Log('py[' + Shown(aText) + ']', 'exc=' + E.ClassName + ' ' + Shown(E.Message));
  end;
end;

procedure Generated;
var
  i, m: Integer;
  s, frac: string;
begin
  // 17-, 18- and 19-digit integers and fractions.
  for i := 1 to 40 do begin
    m := 10000000 + i * 65537;
    ParseOne(IntToStr(m));
    ParseOne(IntToStr(m) + '.' + IntToStr(m * 3));
    ParseOne(IntToStr(m * 137) + '.' + IntToStr(m * 977));
  end;
  // Growing digit counts (1 to 30 digits), each with a fractional tail.
  for i := 1 to 30 do begin
    s := IntToStr(i);
    while Length(s) < i do
      s := s + IntToStr((i * 7 + Length(s)) mod 10);
    frac := IntToStr((i * 131) mod 1000);
    if i <= 10 then begin
      ParseOne(s);
      ParseOne('0.' + s);
      ParseOne(s + '.' + frac);
    end else begin
      ParseOne(s + '.' + frac);
    end;
  end;
  // Exponents across the range.
  for i := -320 to 320 do begin
    if (i >= -40) and (i <= 40) or ((i mod 47) = 0) then begin
      ParseOne('1.5E' + IntToStr(i));
      ParseOne('12345E' + IntToStr(i));
    end;
  end;
  // Exponents at the edges of the double range.
  ParseOne('1E307');
  ParseOne('1E308');
  ParseOne('1.7976931348623157E308');
  ParseOne('1.7976931348623158E308');
  ParseOne('1.8E308');
  ParseOne('1E309');
  ParseOne('1E310');
  ParseOne('1E320');
  ParseOne('1E400');
  ParseOne('1E1000');
  ParseOne('1E9999');
  ParseOne('1E-307');
  ParseOne('1E-308');
  ParseOne('2.2250738585072014E-308');
  ParseOne('1E-324');
  ParseOne('2E-324');
  ParseOne('5E-324');
  ParseOne('1E-325');
  ParseOne('1E-330');
  ParseOne('1E-340');
  ParseOne('1E-400');
  ParseOne('1E-1000');
  ParseOne('1E-9999');
  ParseOne('0.00000000000000000000000000000000000001');
  ParseOne('0.0000000000000000000000000000000000000000000000000001');
  // Blanks, signs and separators.
  ParseOne('  1.5');
  ParseOne('  1.5  ');
  ParseOne(#9 + '1.5');
  ParseOne('1.5' + #9);
  ParseOne(#13 + '1.5');
  ParseOne(#10 + '1.5');
  ParseOne(#0 + '1.5');
  ParseOne('1' + #0 + '.5');
  ParseOne('  +  1.5');
  ParseOne('1 .5');
  ParseOne('e5');
  ParseOne('.e5');
  ParseOne('1e+3.5');
  ParseOne('+0.5');
  ParseOne('-0.5');
  ParseOne('-');
  ParseOne('+');
  ParseOne('.');
  ParseOne('');
  ParseOne(' ');
  ParseOne('1 5');
  ParseOne('1.5x');
  ParseOne('x1.5');
  ParseOne('1,000');
  ParseOne('1.5e+3');
  ParseOne('1.5e-3');
  ParseOne('1.5E+ 3');
  ParseOne('1.5E');
  ParseOne('1.5E+');
  ParseOne('--1.5');
  ParseOne('1..5');
  ParseOne('Infinity');
  ParseOne('infinity');
  ParseOne('INF');
  ParseOne('-Inf');
  ParseOne('+Inf');
  ParseOne('Inf');
  ParseOne('NaN');
  ParseOne('nan');
  ParseOne('-NaN');
  ParseOne('infinityx');
  ParseOne('1e5.5');
end;

function Initialize: integer;
begin
  Result := 0;
  gOut := ScriptsPath;
  gW := TBinaryWriter.Create(TFileStream.Create(gOut + 'numerics-parse.dat', fmCreate));
  Generated;
  gW.Free;
end;

end.
