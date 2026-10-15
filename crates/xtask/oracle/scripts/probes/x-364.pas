{ parity probe: x-364 }
unit P_x_364;

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
  v := TList.Create; v.Add(5); v.Remove(6); AddMessage(IntToStr(v.Count));
  AddMessage('after');
end;
end.
