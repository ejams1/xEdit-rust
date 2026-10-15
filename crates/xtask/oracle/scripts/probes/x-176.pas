{ parity probe: x-176 }
unit P_x_176;

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
  v := Null; v := v or True; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
