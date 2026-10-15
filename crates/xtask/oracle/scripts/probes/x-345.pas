{ parity probe: x-345 }
unit P_x_345;

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
  v := TList.Create; AddMessage(IntToStr(VarType(v.Count)));
  AddMessage('after');
end;
end.
