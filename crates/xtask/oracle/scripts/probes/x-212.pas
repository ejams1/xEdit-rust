{ parity probe: x-212 }
unit P_x_212;

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
  v := sl < sl;
  AddMessage('after');
end;
end.
