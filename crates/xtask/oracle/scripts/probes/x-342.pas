{ parity probe: x-342 }
unit P_x_342;

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
  v := TList.Create; v.Add(sl); AddMessage(TStringList(v[0]).Count);
  AddMessage('after');
end;
end.
