{ parity probe: x-361 }
unit P_x_361;

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
  v := TList.Create; v.Add(5); v.Add(6); v.Move(0, 1); AddMessage(IntToStr(Integer(v[0])));
  AddMessage('after');
end;
end.
