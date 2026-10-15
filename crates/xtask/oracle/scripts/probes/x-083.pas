{ parity probe: x-083 }
unit P_x_083;

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
  d := 0.1 + 0.2; AddMessage(FloatToStr(d));
  AddMessage('after');
end;
end.
