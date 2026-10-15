{ parity probe: x-353 }
unit P_x_353;

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
  v := TList.Create; v.Count := 3; AddMessage(IntToStr(Integer(v[2])));
  AddMessage('after');
end;
end.
