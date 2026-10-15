{ parity probe: x-081 }
unit P_x_081;

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
  d := Power(2, 0.5); AddMessage(FloatToStr(d));
  AddMessage('after');
end;
end.
