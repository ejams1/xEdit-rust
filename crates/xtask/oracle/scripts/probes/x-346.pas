{ parity probe: x-346 }
unit P_x_346;

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
  v := TList.Create; AddMessage(IntToStr(VarType(v.Capacity)));
  AddMessage('after');
end;
end.
