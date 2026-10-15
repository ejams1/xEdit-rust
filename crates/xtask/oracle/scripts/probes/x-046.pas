{ parity probe: x-046 }
unit P_x_046;

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
  sl.Free; i := sl.Count;
  AddMessage('after');
end;
end.
