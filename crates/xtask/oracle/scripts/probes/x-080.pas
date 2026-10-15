{ parity probe: x-080 }
unit P_x_080;

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
  d := Ln(0); AddMessage(FloatToStr(d));
  AddMessage('after');
end;
end.
