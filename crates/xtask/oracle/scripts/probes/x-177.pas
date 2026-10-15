{ parity probe: x-177 }
unit P_x_177;

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
  v := Null; v := v = 1; if v then AddMessage('t') else AddMessage('f');
  AddMessage('after');
end;
end.
