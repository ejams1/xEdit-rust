{ parity probe: x-384 }
unit P_x_384;

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
  v := TList.Create; v.Add(5); v.Items[0] := 6; AddMessage(IntToStr(Integer(v[0])));
  AddMessage('after');
end;
end.
