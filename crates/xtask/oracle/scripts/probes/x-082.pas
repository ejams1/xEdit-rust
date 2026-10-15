{ parity probe: x-082 }
unit P_x_082;

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
  d := 1 / 3; AddMessage(FloatToStr(d));
  AddMessage('after');
end;
end.
