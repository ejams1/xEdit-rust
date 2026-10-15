{ parity probe: x-362 }
unit P_x_362;

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
  v := TList.Create; v.Add(5); v.Insert(0, 6); AddMessage(IntToStr(Integer(v[0])));
  AddMessage('after');
end;
end.
