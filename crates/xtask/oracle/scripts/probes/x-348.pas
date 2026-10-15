{ parity probe: x-348 }
unit P_x_348;

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
  v := TList.Create; v.Add(sl); AddMessage(IntToStr(VarType(v[0])));
  AddMessage('after');
end;
end.
