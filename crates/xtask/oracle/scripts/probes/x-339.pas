{ parity probe: x-339 }
unit P_x_339;

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
  v := TList.Create; v.Add(5); AddMessage(IntToStr(Integer(v[0])));
  AddMessage('after');
end;
end.
