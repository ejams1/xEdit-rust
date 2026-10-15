{ parity probe: x-198 }
unit P_x_198;

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
  v := -Null; AddMessage(VarToStr(v) + ' ' + IntToStr(VarType(v)));
  AddMessage('after');
end;
end.
