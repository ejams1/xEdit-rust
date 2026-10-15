{ parity probe: x-359 }
unit P_x_359;

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
  v := TList.Create; v.Add(5); AddMessage(IntToStr(Integer(v.First)));
  AddMessage('after');
end;
end.
