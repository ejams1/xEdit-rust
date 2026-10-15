{ parity probe: x-001 }
unit P_x_001;

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
  i := 7 div 0;
  AddMessage('after');
end;
end.
