{ parity probe: x-079 }
unit P_x_079;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  d := Sqrt(-1); AddMessage(FloatToStr(d));
  AddMessage('after');
end;
end.
