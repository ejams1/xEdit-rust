{ parity probe: x-186 }
unit P_x_186;

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
  d := True; AddMessage(FloatToStr(d));
  AddMessage('after');
end;
end.
