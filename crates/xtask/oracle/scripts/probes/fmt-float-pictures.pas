{
  Parity fixture (fix-numerics): `FormatFloat`, the RTL's
  `FloatToTextFmt`, over the picture language and its edges.

  The digits come from `FloatToDecimal` (17 significant digits rounded half
  up on the digit string to the picture's decimals, or the picture's digit
  count in scientific notation), so the ties are the RTL's (`0.125` with two
  decimals). The fallback to the general format: an empty picture, one
  that starts with `;`, a value that is not finite, and a value with 18 or
  more integer digits in a picture without `E+`/`E-`. The sections: the
  minus sign is written only when the section used starts the picture, an
  empty section falls back to the first, and the zero section takes NaN.
  Quoted text, a bare `E` without a sign, grouping, the exponent padding
  (at most four zeros) and the exponent of a zero.
}
unit FmtFloatPicturesProbe;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

procedure F(const aPicture: string; aValue: Double);
begin
  try
    Log('ff', '[' + aPicture + '] ' + FloatToStrF(aValue, 1, 17, 0) + ' = [' + FormatFloat(aPicture, aValue) + ']');
  except on E: Exception do
    Log('ff', '[' + aPicture + '] ' + FloatToStrF(aValue, 1, 17, 0) + ' exc=' + E.ClassName + ' ' + E.Message);
  end;
end;

procedure Values(const aPicture: string);
begin
  F(aPicture, 0.0);
  F(aPicture, 1.0);
  F(aPicture, -1.0);
  F(aPicture, 0.125);
  F(aPicture, 0.5);
  F(aPicture, 1.5);
  F(aPicture, 2.5);
  F(aPicture, -2.5);
  F(aPicture, 0.005);
  F(aPicture, 0.0005);
  F(aPicture, 1234567.891);
  F(aPicture, -1234567.891);
  F(aPicture, 0.000012345);
  F(aPicture, 1.0 / 3.0);
  F(aPicture, 14060.7578125);
end;

function Initialize: integer;
begin
  Result := 0;
  Values('0');
  Values('0.00');
  Values('#.##');
  Values('#,##0.00');
  Values('000.000');
  Values('0.0##');
  Values('#');
  Values('#.#');
  Values('.00');
  Values('0.00E+00');
  Values('0.00e-00');
  Values('##0.0E+0');
  Values('0.0E+00000');
  Values('0E-0');
  Values('0;(0)');
  Values('0.0;-0.0;"zero"');
  Values('0;;"z"');
  Values(';0');
  Values('');
  Values('0;');
  Values('"abc"0.0"x"');
  Values('0"abc');
  Values('0E0');
  Values('0,000');
  Values('#,#');
  Values('0.00 %');
  // The general fallback at 18 integer digits.
  F('0', 1.0E16);
  F('0', 99999999999999984.0);
  F('0', 1.0E17);
  F('0', 123456789012345680.0);
  F('0', 1.0E18);
  F('0.#####', 1.0E22);
  F('0.#####', -1.5E22);
  F('0.00E+00', 1.0E22);
  F('#,##0', 1.0E17);
  F('0', 99999999999999999.5);
  // Not finite.
  F('0.00', Infinity);
  F('0.00', NegInfinity);
  F('0.00;(0.00);"zero"', NaN);
  // Grouping of many digits.
  F('#,##0.00', 12345678901234.5);
  F('#,##0.00', -0.004);
  F('#,##0', 999.5);
  F('0.000E+00', 9.9995);
  F('0.000E+00', 0.0);
  F('#.###E-00', 0.0);
end;

end.
