{ parity probe: x-324 }
unit P_x_324;

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
  v := sl.ClassType; AddMessage(IntToStr(VarType(v)));
  AddMessage('after');
end;
end.
