{ parity probe: x-197 }
unit P_x_197;

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
  v := -True; AddMessage(VarToStr(v) + ' ' + IntToStr(VarType(v)));
  AddMessage('after');
end;
end.
