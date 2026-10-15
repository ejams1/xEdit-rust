{ parity probe: x-343 }
unit P_x_343;

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
  v := TList.Create; v.Add(sl); AddMessage(IntToStr(TStringList(v[0]).Count));
  AddMessage('after');
end;
end.
