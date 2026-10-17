{
  Parity fixture (issue #69): the invalid operations of floating point a
  script can reach. xEdit masks the floating point exceptions, so an
  invalid operation answers the default NaN instead of raising
  `EInvalidOp`, as a division by zero answers an infinity.

  Each case logs the value as `FloatToStr` text and whether it is a NaN,
  and writes its eight bytes into `float-invalid.dat` (compared byte for
  byte by the harness), so the sign and payload of each NaN are compared
  too. The variant cases log the result's text and variant type, or the
  exception a raise gives.
}
unit FloatInvalidProbe;

var
  gW: TBinaryWriter;

function Flag(aValue: Boolean): string;
begin
  if aValue then
    Result := 'T'
  else
    Result := 'F';
end;

procedure Value(const aTag: string; aValue: Double);
begin
  gW.Write(aValue);
  AddMessage(aTag + ' | ' + FloatToStr(aValue) + ' nan=' + Flag(IsNan(aValue)));
end;

procedure Raw(const aTag: string; aValue: Variant);
begin
  try
    AddMessage(aTag + ' | ' + VarToStr(aValue) + ' type=' + IntToStr(VarType(aValue)));
  except on E: Exception do
    AddMessage(aTag + ' | exc=' + E.ClassName + ' ' + E.Message);
  end;
end;

procedure Compare(a, b: Double);
var
  t: string;
begin
  t := FloatToStr(a) + ' ' + FloatToStr(b) + ' |';
  try t := t + ' =' + Flag(a = b); except on E: Exception do t := t + ' =exc ' + E.ClassName + ' ' + E.Message; end;
  try t := t + ' <>' + Flag(a <> b); except on E: Exception do t := t + ' <>exc ' + E.ClassName + ' ' + E.Message; end;
  try t := t + ' <' + Flag(a < b); except on E: Exception do t := t + ' <exc ' + E.ClassName + ' ' + E.Message; end;
  try t := t + ' >' + Flag(a > b); except on E: Exception do t := t + ' >exc ' + E.ClassName + ' ' + E.Message; end;
  try t := t + ' <=' + Flag(a <= b); except on E: Exception do t := t + ' <=exc ' + E.ClassName + ' ' + E.Message; end;
  try t := t + ' >=' + Flag(a >= b); except on E: Exception do t := t + ' >=exc ' + E.ClassName + ' ' + E.Message; end;
  AddMessage('cmp | ' + t);
end;

procedure CompareVariants(a, b: Variant);
var
  t: string;
begin
  t := VarToStr(a) + ' ' + VarToStr(b) + ' |';
  try t := t + ' =' + Flag(a = b); except on E: Exception do t := t + ' =exc ' + E.ClassName + ' ' + E.Message; end;
  try t := t + ' <>' + Flag(a <> b); except on E: Exception do t := t + ' <>exc ' + E.ClassName + ' ' + E.Message; end;
  try t := t + ' <' + Flag(a < b); except on E: Exception do t := t + ' <exc ' + E.ClassName + ' ' + E.Message; end;
  try t := t + ' >' + Flag(a > b); except on E: Exception do t := t + ' >exc ' + E.ClassName + ' ' + E.Message; end;
  try t := t + ' <=' + Flag(a <= b); except on E: Exception do t := t + ' <=exc ' + E.ClassName + ' ' + E.Message; end;
  try t := t + ' >=' + Flag(a >= b); except on E: Exception do t := t + ' >=exc ' + E.ClassName + ' ' + E.Message; end;
  AddMessage('vcmp | ' + t);
end;

procedure Cases;
var
  z, one, inf, ninf, d: Double;
  i, j: Integer;
  v, w: Variant;
  sl: TStringList;
begin
  z := 0.0;
  one := 1.0;
  inf := 1.0E308;
  inf := inf * 10.0;
  ninf := -inf;
  Value('0.0/0.0', z / z);
  Value('-0.0/0.0', (-z) / z);
  Value('inf-inf', inf - inf);
  Value('inf+ninf', inf + ninf);
  Value('inf*0', inf * z);
  Value('0*ninf', z * ninf);
  Value('inf/inf', inf / inf);
  Value('ninf/inf', ninf / inf);
  Value('1/0', one / z);
  Value('-1/0', -one / z);
  d := z / z;
  Value('stored', d);
  Value('nan+1', d + 1);
  Value('nan*0', d * 0);
  Value('-nan', -d);
  Value('sqrt(-1)', Sqrt(-one));
  Value('ln(-1)', Ln(-one));
  Value('ln(0)', Ln(z));
  i := 0;
  j := 0;
  Value('int0/int0', i / j);
  Value('lit 0/0', 0 / 0);
  Value('lit 0.0/0', 0.0 / 0);
  v := 0.0;
  w := 0;
  Raw('var 0.0/0', v / w);
  Raw('var 0/0', w / w);
  Raw('var str', '0' / w);
  Compare(d, d);
  Compare(d, 1.0);
  Compare(1.0, d);
  Compare(d, z / z);
  Compare(d, inf);
  Compare(inf, inf);
  Compare(ninf, inf);
  v := d;
  w := 1;
  CompareVariants(v, w);
  CompareVariants(v, v);
  w := 'abc';
  CompareVariants(v, w);
  w := '1';
  CompareVariants(v, w);
  if d = d then
    AddMessage('if | nan = nan')
  else
    AddMessage('if | not nan = nan');
  if d < 1 then
    AddMessage('if | nan < 1')
  else
    AddMessage('if | not nan < 1');
  if d > 1 then
    AddMessage('if | nan > 1')
  else
    AddMessage('if | not nan > 1');
  if (d >= 1) or (d <= 1) then
    AddMessage('if | nan >= 1 or nan <= 1')
  else
    AddMessage('if | neither');
  i := 0;
  while (i < 3) and not (d = d) do
    i := i + 1;
  AddMessage('while | ' + IntToStr(i));
  try AddMessage('max | ' + FloatToStr(Max(d, 1.0)) + ' ' + FloatToStr(Max(1.0, d))); except on E: Exception do AddMessage('max | exc ' + E.ClassName + ' ' + E.Message); end;
  try AddMessage('min | ' + FloatToStr(Min(d, 1.0)) + ' ' + FloatToStr(Min(1.0, d))); except on E: Exception do AddMessage('min | exc ' + E.ClassName + ' ' + E.Message); end;
  try AddMessage('compare | ' + IntToStr(CompareValue(d, 1.0)) + ' ' + IntToStr(CompareValue(1.0, d)) + ' ' + IntToStr(CompareValue(d, d))); except on E: Exception do AddMessage('compare | exc ' + E.ClassName + ' ' + E.Message); end;
  try AddMessage('samevalue | ' + Flag(SameValue(d, d)) + Flag(SameValue(d, 1.0))); except on E: Exception do AddMessage('samevalue | exc ' + E.ClassName + ' ' + E.Message); end;
  try
    v := d;
    case Round(1) of
      1: AddMessage('case | one');
    end;
    sl := TStringList.Create;
    sl.Add(FloatToStr(d));
    sl.Add(FloatToStr(1.0));
    AddMessage('list | ' + sl.CommaText);
    sl.Free;
  except on E: Exception do AddMessage('tail | exc ' + E.ClassName + ' ' + E.Message); end;
  try
    AddMessage('round | ' + IntToStr(Round(z / z)));
  except on E: Exception do
    AddMessage('round | exc=' + E.ClassName + ' ' + E.Message);
  end;
  try
    AddMessage('trunc | ' + IntToStr(Trunc(z / z)));
  except on E: Exception do
    AddMessage('trunc | exc=' + E.ClassName + ' ' + E.Message);
  end;
end;

function Initialize: integer;
begin
  Result := 0;
  gW := TBinaryWriter.Create(TFileStream.Create(ScriptsPath + 'float-invalid.dat', fmCreate));
  Cases;
  gW.Free;
  AddMessage('done');
end;

end.
