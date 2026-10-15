{ parity probe: x-323 }
unit P_x_323;

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
  v := sl.ClassName; AddMessage(v);
  AddMessage('after');
end;
end.
